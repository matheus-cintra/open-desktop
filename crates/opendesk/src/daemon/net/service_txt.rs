use std::collections::HashMap;

use opendesk_proto::control::PeerId;

pub const TXT_ID: &str = "id";
pub const TXT_NAME: &str = "name";
pub const TXT_VERSION: &str = "version";
pub const TXT_PORT: &str = "port";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTxt {
    pub peer_id: PeerId,
    pub name: String,
    pub version: String,
    pub port: u16,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ServiceTxtError {
    #[error("txt record is missing `{0}`")]
    Missing(&'static str),
    #[error("txt record has an invalid `{0}`")]
    Invalid(&'static str),
}

pub fn build_txt(service: &ServiceTxt) -> HashMap<String, String> {
    HashMap::from([
        (TXT_ID.to_owned(), service.peer_id.to_hex()),
        (TXT_NAME.to_owned(), service.name.clone()),
        (TXT_VERSION.to_owned(), service.version.clone()),
        (TXT_PORT.to_owned(), service.port.to_string()),
    ])
}

pub fn parse_txt<'a>(
    lookup: impl Fn(&str) -> Option<&'a str>,
) -> Result<ServiceTxt, ServiceTxtError> {
    let peer_id = PeerId::parse_hex(lookup(TXT_ID).ok_or(ServiceTxtError::Missing(TXT_ID))?)
        .map_err(|_| ServiceTxtError::Invalid(TXT_ID))?;
    let name = lookup(TXT_NAME)
        .ok_or(ServiceTxtError::Missing(TXT_NAME))?
        .to_owned();
    let version = lookup(TXT_VERSION)
        .ok_or(ServiceTxtError::Missing(TXT_VERSION))?
        .to_owned();
    let port = lookup(TXT_PORT)
        .ok_or(ServiceTxtError::Missing(TXT_PORT))?
        .parse::<u16>()
        .map_err(|_| ServiceTxtError::Invalid(TXT_PORT))?;
    Ok(ServiceTxt {
        peer_id,
        name,
        version,
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ServiceTxt {
        ServiceTxt {
            peer_id: PeerId([0x5a; 16]),
            name: "notebook".to_owned(),
            version: "0.1.0".to_owned(),
            port: 47820,
        }
    }

    #[test]
    fn txt_round_trips() {
        let map = build_txt(&sample());
        assert_eq!(map.len(), 4);
        assert_eq!(map[TXT_ID], "5a".repeat(16));
        let parsed = parse_txt(|key| map.get(key).map(String::as_str)).unwrap();
        assert_eq!(parsed, sample());
    }

    #[test]
    fn missing_and_invalid_fields_are_reported() {
        let mut map = build_txt(&sample());
        map.remove(TXT_NAME);
        assert_eq!(
            parse_txt(|key| map.get(key).map(String::as_str)),
            Err(ServiceTxtError::Missing(TXT_NAME))
        );
        let mut map = build_txt(&sample());
        map.insert(TXT_PORT.to_owned(), "not-a-port".to_owned());
        assert_eq!(
            parse_txt(|key| map.get(key).map(String::as_str)),
            Err(ServiceTxtError::Invalid(TXT_PORT))
        );
        let mut map = build_txt(&sample());
        map.insert(TXT_ID.to_owned(), "abc".to_owned());
        assert_eq!(
            parse_txt(|key| map.get(key).map(String::as_str)),
            Err(ServiceTxtError::Invalid(TXT_ID))
        );
    }
}
