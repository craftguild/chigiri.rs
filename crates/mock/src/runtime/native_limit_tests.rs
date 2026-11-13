//! External watchdog evidence for bounded VM-native capabilities.

use std::sync::mpsc;
use std::time::Duration;

use chigiri::core::Object;

use super::tests::{contract, run, script};
use crate::output::OutputValue;
use crate::store::Store;

#[test]
fn sa_02_native_limits_are_sticky_and_a_fresh_vm_recovers_under_a_watchdog() {
    let _case = crate::vm_gate::test_case();
    let pattern = "a".repeat(257);
    let cases = vec![
        format!(
            "pcall(function() string.find('text', '{pattern}') end) return 1"
        ),
        "local v={} for i=1,1001 do v[i]=i end pcall(function() table.sort(v) end) return 1"
            .into(),
        "local v={b=2,a=1} for i=1,1001 do v['k'..tostring(i)]=i end pcall(function() table.clone(v) end) return 1"
            .into(),
        "local v={2,1} pcall(function() table.sort(v,function() table.sort(v) return false end) end) return 1"
            .into(),
        "pcall(function() local s='x' for _=1,30 do s=s..s end end) return 1".into(),
    ];
    watchdog(move || {
        let contract = contract("type Query{value:Int!}");
        for body in cases {
            let failed = script(&contract, "Query.value", &body);
            let mut store = Store::empty(contract.ir().clone());
            assert!(run(&failed, &Object::new(), &mut store).is_err(), "{body}");
            let good = script(&contract, "Query.value", "return 7");
            assert!(matches!(
                run(&good, &Object::new(), &mut store).unwrap(),
                OutputValue::Scalar(chigiri::core::Value::Int(7))
            ));
        }
    });
}

#[test]
fn sa_02_pattern_and_sort_callback_finish_under_external_watchdogs() {
    let _case = crate::vm_gate::test_case();
    let text = "a".repeat(crate::value::MAX_STRING_BYTES);
    let pattern = format!("{}z", ".-".repeat(100));
    watchdog(move || {
        let contract = contract("type Query{value:Int!}");
        let pattern_call =
            format!("local first = string.find('{text}', '{pattern}') return first or 0");
        let pattern_script = script(&contract, "Query.value", &pattern_call);
        let mut store = Store::empty(contract.ir().clone());
        let _ = run(&pattern_script, &Object::new(), &mut store);

        let callback = script(
            &contract,
            "Query.value",
            "local v={2,1} table.sort(v,function() while true do end return false end) return 1",
        );
        assert!(run(&callback, &Object::new(), &mut store).is_err());
        let good = script(&contract, "Query.value", "return 9");
        assert!(matches!(
            run(&good, &Object::new(), &mut store).unwrap(),
            OutputValue::Scalar(chigiri::core::Value::Int(9))
        ));
    });
}

#[test]
fn tl_15_os_format_and_date_table_limits_recover_under_a_watchdog() {
    let _case = crate::vm_gate::test_case();
    let chunk = "x".repeat(512);
    let directives = "%c".repeat(32);
    let escaped = "%%".repeat(129);
    let cases = vec![
        (
            format!(
                "local value=assert(os.date('!{escaped}',0)); assert(type(value)=='string'); return string.len(value)"
            ),
            true,
            "literal percent directives",
        ),
        (
            format!(
                "local parts:any={{}}; for i=1,8 do parts[i]='{chunk}' end; local f=string.sub(table.concat(parts),1,4095); local _value=os.date('!'..f,0); return 1"
            ),
            true,
            "4096-byte format",
        ),
        (
            format!(
                "local parts:any={{}}; for i=1,8 do parts[i]='{chunk}' end; local f=table.concat(parts); pcall(function() local _value=os.date('!'..f,0) end); return 1"
            ),
            false,
            "4097-byte format",
        ),
        (
            format!(
                "local parts:any={{}}; for i=1,4 do parts[i]='{directives}' end; local f=string.sub(table.concat(parts),1,254); local _value=os.date('!'..f,0); return 1"
            ),
            true,
            "bounded expansion",
        ),
        (
            format!(
                "local parts:any={{}}; for i=1,4 do parts[i]='{directives}' end; local f=table.concat(parts); pcall(function() local _value=os.date('!'..f,0) end); return 1"
            ),
            false,
            "oversized expansion",
        ),
    ];
    watchdog(move || {
        let contract = contract("type Query{value:Int!}");
        let mut store = Store::empty(contract.ir().clone());
        for (body, succeeds, case) in cases {
            let resolver = script(&contract, "Query.value", &body);
            assert_eq!(
                run(&resolver, &Object::new(), &mut store).is_ok(),
                succeeds,
                "{case}"
            );
        }
        let table_limit = script(
            &contract,
            "Query.value",
            "local value:any={year=1970,month=1,day=1}; for i=1,1001 do value['x'..i]=i end; pcall(function() os.time(value) end); return 1",
        );
        assert!(run(&table_limit, &Object::new(), &mut store).is_err());
        for value in ["1e300", "0/0", "'999999999999999999999'"] {
            let body = format!(
                "local value:any={{year={value},month=1,day=1}}; pcall(function() os.time(value) end); return 1"
            );
            let unsafe_field = script(&contract, "Query.value", &body);
            assert!(run(&unsafe_field, &Object::new(), &mut store).is_err());
        }
        for value in ["1e300", "0/0", "'999999999999999999999'"] {
            let body = format!(
                "local date:any=os.date; local value:any={value}; pcall(function() date('!%Y',value) end); return 1"
            );
            let unsafe_timestamp = script(&contract, "Query.value", &body);
            assert!(run(&unsafe_timestamp, &Object::new(), &mut store).is_err());
        }
        let good = script(
            &contract,
            "Query.value",
            "return assert(os.time({year=1970,month=1,day=1,hour=0}))",
        );
        assert!(matches!(
            run(&good, &Object::new(), &mut store).unwrap(),
            OutputValue::Scalar(chigiri::core::Value::Int(0))
        ));
    });
}

fn watchdog(work: impl FnOnce() + Send + 'static) {
    let (sent, received) = mpsc::sync_channel(1);
    let thread = std::thread::Builder::new()
        .name("chigiri-native-watchdog".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            work();
            sent.send(()).unwrap();
        })
        .unwrap();
    received
        .recv_timeout(Duration::from_secs(15))
        .expect("bounded native operation exceeded its external watchdog");
    thread.join().unwrap();
}
