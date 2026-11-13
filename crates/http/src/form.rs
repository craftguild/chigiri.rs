//! Decodes bounded `application/x-www-form-urlencoded` GraphQL GET queries.

use std::collections::BTreeMap;

pub(crate) fn decode(input: &str) -> Result<BTreeMap<String, String>, ()> {
    let mut output = BTreeMap::new();
    if input.is_empty() {
        return Ok(output);
    }
    for pair in input.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = component(name)?;
        let value = component(value)?;
        if matches!(
            name.as_str(),
            "query" | "operationName" | "variables" | "extensions"
        ) && output.insert(name, value).is_some()
        {
            return Err(());
        }
    }
    Ok(output)
}

fn component(input: &str) -> Result<String, ()> {
    let mut bytes = Vec::with_capacity(input.len());
    let input = input.as_bytes();
    let mut index = 0;
    while index < input.len() {
        match input[index] {
            b'+' => bytes.push(b' '),
            b'%' => {
                let high = *input.get(index + 1).ok_or(())?;
                let low = *input.get(index + 2).ok_or(())?;
                bytes.push((hex(high)? << 4) | hex(low)?);
                index += 2;
            }
            byte => bytes.push(byte),
        }
        index += 1;
    }
    String::from_utf8(bytes).map_err(|_| ())
}

fn hex(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn decodes_percent_escapes_and_plus_signs() {
        let decoded = decode("query=%7Bbook%7D&operationName=A+B").unwrap();
        assert_eq!(decoded["query"], "{book}");
        assert_eq!(decoded["operationName"], "A B");
    }

    #[test]
    fn rejects_invalid_percent_encoding_utf8_and_duplicate_known_parameters() {
        assert!(decode("query=%GG").is_err());
        assert!(decode("query=%FF").is_err());
        assert!(decode("query=a&query=b").is_err());
    }
}
