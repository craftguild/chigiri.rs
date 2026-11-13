//! Owns real-binary startup, shutdown, and worker recovery.

use super::{BINARY, Directory, PREPARATION_WATCHDOG, Response, offline, request};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct Server {
    pub directory: Directory,
    child: Child,
    pub address: SocketAddr,
}

impl Server {
    pub fn start(config: &str) -> Self {
        Self::launch(config, false, false)
    }

    pub fn offline(config: &str) -> Self {
        Self::launch(config, true, false)
    }

    pub fn project(directory: Directory) -> Self {
        Self::spawn(
            directory,
            Command::new(BINARY),
            "--config-dir",
            "mock",
            false,
        )
    }

    pub fn with_graphiql(config: &str, offline: bool) -> Self {
        Self::launch(config, offline, true)
    }

    pub fn project_with_graphiql(directory: Directory) -> Self {
        Self::spawn(
            directory,
            Command::new(BINARY),
            "--config-dir",
            "mock",
            true,
        )
    }

    fn launch(config: &str, offline: bool, graphiql: bool) -> Self {
        let directory = Directory::new();
        fs::write(
            directory.0.join("mock.toml"),
            config.replace("127.0.0.1:4000", "127.0.0.1:0"),
        )
        .unwrap();
        let command = if offline {
            offline::command(&directory)
        } else {
            Command::new(BINARY)
        };
        Self::spawn(directory, command, "--config", "mock.toml", graphiql)
    }

    fn spawn(
        directory: Directory,
        mut command: Command,
        flag: &str,
        path: &str,
        graphiql: bool,
    ) -> Self {
        command.args(["mock", "serve", flag, path]);
        if graphiql {
            command.arg("--enable-graphiql");
        }
        let child = command
            .current_dir(&directory.0)
            .stdin(Stdio::null())
            .stdout(File::create(directory.0.join("stdout.log")).unwrap())
            .stderr(File::create(directory.0.join("stderr.log")).unwrap())
            .spawn()
            .unwrap();
        let mut server = Self {
            directory,
            child,
            address: "127.0.0.1:0".parse().unwrap(),
        };
        let deadline = Instant::now() + PREPARATION_WATCHDOG;
        loop {
            let stderr = fs::read_to_string(server.directory.0.join("stderr.log")).unwrap();
            if let Some(address) = startup_address(&stderr, graphiql) {
                server.address = address;
                assert_ne!(server.address.port(), 0);
                return server;
            }
            if let Some(status) = server.child.try_wait().unwrap() {
                panic!(
                    "server startup failed {status}: {}",
                    fs::read_to_string(server.directory.0.join("stderr.log")).unwrap()
                );
            }
            assert!(
                Instant::now() < deadline,
                "server startup watchdog expired (GraphiQL={graphiql}): {stderr}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn graphql(&self, query: &str) -> Value {
        self.operation(json!({"query": query}))
    }

    pub fn operation(&self, operation: Value) -> Value {
        let response = self.http(
            "POST",
            &[("Content-Type", "application/json")],
            &operation.to_string(),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        serde_json::from_str(&response.body).unwrap()
    }

    pub fn http(&self, method: &str, headers: &[(&str, &str)], body: &str) -> Response {
        request(self.address, method, headers, body)
    }

    pub fn stop(&mut self, signal: &str) {
        assert!(
            Command::new("kill")
                .args([signal, &self.child.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "server exit: {status}");
                let stderr = fs::read_to_string(self.directory.0.join("stderr.log")).unwrap();
                assert!(
                    shutdown_notice_observed(&stderr),
                    "successful shutdown lacks a join notice or attributed notice loss: {stderr}"
                );
                assert!(
                    TcpStream::connect_timeout(&self.address, Duration::from_millis(100)).is_err()
                );
                println!("worker joined after {signal}: {:?}", started.elapsed());
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "shutdown watchdog expired"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn startup_address(stderr: &str, graphiql: bool) -> Option<SocketAddr> {
    let address: SocketAddr = stderr
        .lines()
        .find_map(|line| line.strip_prefix("chigiri mock listening http://"))
        .and_then(|line| line.strip_suffix("/graphql"))
        .map(|address| address.parse().unwrap())?;
    // These notices use separate asynchronous writer frames, so observing the
    // listening address does not establish that the GraphiQL notice arrived.
    let notice =
        format!("chigiri mock GraphiQL http://{address}/graphiql (schema introspection is public)");
    if graphiql && !stderr.lines().any(|line| line == notice) {
        return None;
    }
    Some(address)
}

#[test]
fn startup_readiness_waits_for_complete_graphiql_notice() {
    for address in ["127.0.0.1:43210", "[::1]:43210"] {
        let listening = format!("chigiri mock listening http://{address}/graphql\n");
        let partial = format!("{listening}chigiri mock GraphiQL http://{address}/graphiql");
        let complete = format!("{partial} (schema introspection is public)\n");
        let address = address.parse().unwrap();
        assert_eq!(startup_address(&listening, true), None);
        assert_eq!(startup_address(&partial, true), None);
        assert_eq!(
            startup_address(
                &format!(
                    "{listening}chigiri mock GraphiQL http://127.0.0.1:1/graphiql (schema introspection is public)\n"
                ),
                true
            ),
            None
        );
        assert_eq!(startup_address(&complete, true), Some(address));
        assert_eq!(startup_address(&listening, false), Some(address));
    }
}

fn shutdown_notice_observed(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        if line == "chigiri mock worker joined" {
            return true;
        }
        let Some(fields) = line.strip_prefix("chigiri mock log loss: ") else {
            return false;
        };
        let mut fields = fields.split_whitespace();
        let counters_valid = ["hook=", "access=", "subscription="]
            .into_iter()
            .all(|prefix| {
                fields
                    .next()
                    .and_then(|field| loss_count(field, prefix))
                    .is_some()
            });
        counters_valid
            && fields
                .next()
                .and_then(|field| loss_count(field, "notice="))
                .is_some_and(|count| count > 0)
            && fields.next().is_none()
    })
}

fn loss_count(field: &str, prefix: &str) -> Option<u64> {
    let text = field.strip_prefix(prefix)?;
    let count = text.parse::<u64>().ok()?;
    (count.to_string() == text).then_some(count)
}

#[test]
fn shutdown_notice_requires_join_or_explicit_notice_loss() {
    for notice in [
        "chigiri mock worker joined\n",
        "chigiri mock log loss: hook=0 access=0 subscription=0 notice=28\n",
    ] {
        assert!(shutdown_notice_observed(notice), "{notice}");
    }
    for notice in [
        "",
        "chigiri mock log loss: hook=1 access=2 subscription=3 notice=0",
        "chigiri mock log loss: hook=0 access=0 subscription=0 notice=-1",
        "chigiri mock log loss: hook=0 access=0 subscription=0 notice=+28",
        "chigiri mock log loss: hook=0 access=0 subscription=0 notice=028",
        "chigiri mock log loss: hook=0 access=0 subscription=0 notice=28 extra=1",
        "chigiri mock log loss: notice=28",
        "{\"message\":\"chigiri mock worker joined\"}",
    ] {
        assert!(!shutdown_notice_observed(notice), "{notice}");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
