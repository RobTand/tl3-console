//! `POST /api/requests/{id}/commands` (PRD #327, C3).

use axum::http::HeaderMap;

/// Browsers never send this header cross-site without a CORS preflight,
/// which the console never grants.
pub const HEADER: &str = "x-tl3-console";

/// The write came from this console's own page.
#[allow(dead_code)]
fn same_origin(_headers: &HeaderMap) -> bool {
    todo!()
}

#[allow(dead_code)]
fn is_uuid(_id: &str) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_static(value));
        }
        headers
    }

    #[test]
    fn writes_need_the_console_header_and_the_same_origin() {
        let own = [("host", "192.168.1.107:8034"), (HEADER, "1")];
        assert!(same_origin(&headers(&own)));
        assert!(same_origin(&headers(&[
            own[0],
            own[1],
            ("origin", "http://192.168.1.107:8034"),
            ("sec-fetch-site", "same-origin"),
        ])));
        for case in [
            vec![own[0]],
            vec![own[0], (HEADER, "0")],
            vec![own[0], own[1], ("origin", "http://evil.example")],
            vec![
                own[0],
                own[1],
                ("origin", "http://192.168.1.107:8034.evil.example"),
            ],
            vec![own[0], own[1], ("origin", "null")],
            vec![own[0], own[1], ("sec-fetch-site", "cross-site")],
            vec![own[0], own[1], ("sec-fetch-site", "same-site")],
            vec![own[1], ("origin", "http://192.168.1.107:8034")],
        ] {
            assert!(!same_origin(&headers(&case)), "{case:?}");
        }
    }

    #[test]
    fn ids_are_lowercase_uuids() {
        assert!(is_uuid("0b9f3c3e-5f43-4a8e-9d1c-2f0e7c1d9a11"));
        for id in [
            "",
            "0B9F3C3E-5F43-4A8E-9D1C-2F0E7C1D9A11",
            "0b9f3c3e5f434a8e9d1c2f0e7c1d9a11",
            "0b9f3c3e-5f43-4a8e-9d1c-2f0e7c1d9a1",
            "0b9f3c3e-5f43-4a8e-9d1c-2f0e7c1d9a1g",
        ] {
            assert!(!is_uuid(id), "{id}");
        }
    }
}
