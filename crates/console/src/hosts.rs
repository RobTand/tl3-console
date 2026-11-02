//! DNS rebinding: a page on another site can point its own host name at the
//! console's address, and the browser then treats the console as that site,
//! Origin and all. The console answers only to the Host names it serves.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;

/// The Host header values this console answers to.
#[derive(Clone, Debug)]
pub(crate) struct Hosts;

impl Hosts {
    /// The bound address, `localhost` on a loopback address, and public names
    /// such as the tunnel's (#338).
    pub(crate) fn new(_address: SocketAddr, _public: &[String]) -> crate::Result<Self> {
        Ok(Self)
    }

    pub(crate) fn allows(&self, _host: Option<&str>) -> bool {
        todo!()
    }
}

/// Refuse any request, read or write, for a Host this console doesn't serve.
pub(crate) async fn guard(State(_hosts): State<Hosts>, request: Request, next: Next) -> Response {
    next.run(request).await
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
        for bad in [
            "",
            "http://console.example.test",
            "console.example.test/x",
            "a b",
            "console.example.test\r\n",
        ] {
            assert!(
                Hosts::new("127.0.0.1:9100".parse().unwrap(), &[bad.to_string()]).is_err(),
                "{bad:?}"
            );
        }
    }
}
