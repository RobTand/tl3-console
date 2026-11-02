//! DNS rebinding: a page on another site can point its own host name at the
//! console's address, and the browser then treats the console as that site,
//! Origin and all. The console answers only to the Host names it serves.

use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{collections::BTreeSet, net::SocketAddr, sync::Arc};

/// The Host header values this console answers to, lowercased.
#[derive(Clone, Debug)]
pub(crate) struct Hosts(Arc<BTreeSet<String>>);

/// Why a public host name is refused, for the config's error message.
pub(crate) const NAME_RULE: &str =
    "use host names with an optional port, such as console.example.test or 192.168.1.107:8034";

/// A public host name is letters, digits and `.-:[]`, so a header value or a
/// URL can't pass for one.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

impl Hosts {
    /// The bound address, `localhost` on a loopback address, and the public
    /// names the config lists, such as a tunnel's.
    pub(crate) fn new(address: SocketAddr, public: &[String]) -> crate::Result<Self> {
        let mut names = BTreeSet::from([address.to_string()]);
        if address.ip().is_loopback() {
            names.insert(format!("localhost:{}", address.port()));
        }
        for name in public {
            if !valid_name(name) {
                return Err(crate::Error::Invalid(NAME_RULE));
            }
            names.insert(name.to_ascii_lowercase());
        }
        Ok(Self(Arc::new(names)))
    }

    pub(crate) fn allows(&self, host: Option<&str>) -> bool {
        host.is_some_and(|host| self.0.contains(&host.to_ascii_lowercase()))
    }
}

/// Refuse any request, read or write, for a Host this console doesn't serve.
pub(crate) async fn guard(State(hosts): State<Hosts>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok());
    if hosts.allows(host) {
        next.run(request).await
    } else {
        (
            StatusCode::MISDIRECTED_REQUEST,
            "This console doesn't answer to that host name.\n",
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(address: &str, public: &[&str]) -> Hosts {
        let public: Vec<_> = public.iter().map(|host| host.to_string()).collect();
        Hosts::new(address.parse().unwrap(), &public).unwrap()
    }

    #[test]
    fn answers_to_its_address_and_configured_names_only() {
        let lan = hosts("192.168.1.107:8034", &["Console.Example.Test"]);
        for host in [
            "192.168.1.107:8034",
            "console.example.test",
            "CONSOLE.example.test",
        ] {
            assert!(lan.allows(Some(host)), "{host}");
        }
        for host in [
            None,
            Some(""),
            Some("192.168.1.107"),
            Some("192.168.1.107:8035"),
            Some("localhost:8034"),
            Some("evil.example:8034"),
            Some("console.example.test.evil.example"),
        ] {
            assert!(!lan.allows(host), "{host:?}");
        }
        let loopback = hosts("127.0.0.1:9100", &[]);
        assert!(loopback.allows(Some("127.0.0.1:9100")));
        assert!(loopback.allows(Some("localhost:9100")));
        assert!(!loopback.allows(Some("localhost")));
        let v6 = hosts("[::1]:9100", &[]);
        assert!(v6.allows(Some("[::1]:9100")));
        assert!(v6.allows(Some("localhost:9100")));
    }

    #[test]
    fn public_names_are_host_names() {
        for good in ["console.example.test", "192.168.1.107:8034", "[::1]:8034"] {
            assert!(valid_name(good), "{good:?}");
        }
        for bad in [
            "",
            "http://console.example.test",
            "console.example.test/x",
            "a b",
            "console.example.test\r\n",
        ] {
            assert!(!valid_name(bad), "{bad:?}");
            assert!(
                Hosts::new("127.0.0.1:9100".parse().unwrap(), &[bad.to_string()]).is_err(),
                "{bad:?}"
            );
        }
    }
}
