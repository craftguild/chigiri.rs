//! HTML-escaped path configuration and same-origin response headers for GraphiQL.

use super::Paths;
use crate::{ResponseHeader, ResponseParts};

// Monaco and Radix require inline styles. Scripts, eval, blob workers, and
// external origins remain prohibited.
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; worker-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub(super) fn html(paths: &Paths, head: bool) -> ResponseParts {
    let endpoint = escape(paths.graphql());
    let assets = escape(paths.assets_prefix());
    let body = format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>Chigiri GraphiQL</title>\n\
         <link rel=\"stylesheet\" href=\"{assets}app.css\">\n\
         <link rel=\"license\" href=\"{assets}THIRD_PARTY_NOTICES.txt\">\n\
         <script type=\"module\" src=\"{assets}app.js\"></script>\n\
         </head>\n<body>\n<div id=\"root\" data-endpoint=\"{endpoint}\"></div>\n\
         <noscript>GraphiQL requires JavaScript.</noscript>\n</body>\n</html>\n"
    );
    content("text/html; charset=utf-8", body.as_bytes(), head)
}

pub(super) fn content(content_type: &str, body: &[u8], head: bool) -> ResponseParts {
    ResponseParts {
        status: 200,
        headers: vec![
            header("content-type", content_type),
            header("content-length", &body.len().to_string()),
            header("cache-control", "no-store"),
            header("x-content-type-options", "nosniff"),
            header("content-security-policy", CSP),
            header("referrer-policy", "no-referrer"),
        ],
        body: if head { Vec::new() } else { body.to_vec() },
    }
}

pub(super) fn not_found() -> ResponseParts {
    let mut response = content("text/plain; charset=utf-8", b"", false);
    response.status = 404;
    response
}

pub(super) fn method_not_allowed() -> ResponseParts {
    let mut response = not_found();
    response.status = 405;
    response.headers.push(header("allow", "GET, HEAD"));
    response
}

fn header(name: &str, value: &str) -> ResponseHeader {
    ResponseHeader {
        name: name.into(),
        value: value.into(),
    }
}

fn escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(ch),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    #[test]
    fn dx35_14_23_attribute_escape_cannot_create_script_or_another_attribute() {
        assert_eq!(
            super::escape("\"'><script>&"),
            "&quot;&#39;&gt;&lt;script&gt;&amp;"
        );
    }
}
