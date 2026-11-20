//! Minimal S3-compatible client for Cloudflare R2: PUT (with If-Match / If-None-Match), GET and
//! ListObjectsV2, signed with AWS Signature V4. Path-style URLs: `{endpoint}/{bucket}/{key}`.
//!
//! Configured from `RFOPS_R2_ENDPOINT` (`https://<account>.r2.cloudflarestorage.com`),
//! `RFOPS_R2_BUCKET`, `RFOPS_R2_ACCESS_KEY_ID` and `RFOPS_R2_SECRET_ACCESS_KEY`.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct R2 {
    endpoint: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    region: String,
    http: reqwest::Client,
}

#[derive(Debug)]
pub enum R2Error {
    /// The precondition (If-Match / If-None-Match) failed: someone else wrote first.
    PreconditionFailed,
    Status(u16, String),
    Network(String),
}

impl std::fmt::Display for R2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            R2Error::PreconditionFailed => write!(f, "precondition failed (412)"),
            R2Error::Status(s, b) => write!(f, "R2 answered {s}: {}", b.chars().take(200).collect::<String>()),
            R2Error::Network(e) => write!(f, "R2 unreachable: {e}"),
        }
    }
}

/// A write condition.
#[derive(Debug, Clone)]
pub enum Cond {
    None,
    /// Only if the object's current ETag matches.
    IfMatch(String),
    /// Only if the object does not exist.
    IfNoneMatch,
}

#[derive(Debug, Clone)]
pub struct Listed {
    pub key: String,
    pub last_modified: String,
    pub size: u64,
}

fn sha256_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key).expect("any key length");
    m.update(data.as_bytes());
    m.finalize().into_bytes().to_vec()
}

/// RFC 3986 encoding as S3 expects (`/` kept in paths when `keep_slash`).
fn uri_encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// AWS Signature V4 `Authorization` header for a request.
/// `headers` must include `host`, `x-amz-content-sha256` and `x-amz-date` (lower-case names).
#[allow(clippy::too_many_arguments)]
pub fn sign_v4(
    access_key: &str,
    secret_key: &str,
    region: &str,
    method: &str,
    canonical_uri: &str,
    query: &[(String, String)],
    headers: &[(String, String)],
    payload_hash: &str,
    amz_date: &str,
) -> String {
    let date = &amz_date[..8];
    let mut q: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k, false), uri_encode(v, false)))
        .collect();
    q.sort();
    let canonical_query = q
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let mut h: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.trim().to_string()))
        .collect();
    h.sort();
    let canonical_headers: String = h.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers = h.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(";");
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!("{date}/{region}/s3/aws4_request");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let k_date = hmac(format!("AWS4{secret_key}").as_bytes(), date);
    let k_region = hmac(&k_date, region);
    let k_service = hmac(&k_region, "s3");
    let k_signing = hmac(&k_service, "aws4_request");
    let signature = hex::encode(hmac(&k_signing, &to_sign));
    format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

impl R2 {
    pub fn from_env() -> Option<Self> {
        let v = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Some(Self::new(
            &v("RFOPS_R2_ENDPOINT")?,
            &v("RFOPS_R2_BUCKET")?,
            &v("RFOPS_R2_ACCESS_KEY_ID")?,
            &v("RFOPS_R2_SECRET_ACCESS_KEY")?,
        ))
    }

    pub fn new(endpoint: &str, bucket: &str, access_key: &str, secret_key: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            bucket: bucket.to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            region: "auto".into(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("http client"),
        }
    }

    fn host(&self) -> String {
        self.endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string()
    }

    async fn send(
        &self,
        method: &str,
        key: Option<&str>,
        query: &[(String, String)],
        body: Vec<u8>,
        extra: &[(String, String)],
    ) -> Result<reqwest::Response, R2Error> {
        let path = match key {
            Some(k) => format!("/{}/{}", self.bucket, k),
            None => format!("/{}", self.bucket),
        };
        let canonical_uri = uri_encode(&path, true);
        let payload_hash = sha256_hex(&body);
        let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let mut headers = vec![
            ("host".to_string(), self.host()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        headers.extend(extra.iter().cloned());
        let auth = sign_v4(
            &self.access_key,
            &self.secret_key,
            &self.region,
            method,
            &canonical_uri,
            query,
            &headers,
            &payload_hash,
            &amz_date,
        );
        let qs = query
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k, false), uri_encode(v, false)))
            .collect::<Vec<_>>()
            .join("&");
        let url = if qs.is_empty() {
            format!("{}{canonical_uri}", self.endpoint)
        } else {
            format!("{}{canonical_uri}?{qs}", self.endpoint)
        };
        let m = reqwest::Method::from_bytes(method.as_bytes()).expect("valid method");
        let mut req = self.http.request(m, url).header("authorization", auth);
        for (k, v) in &headers {
            if k != "host" {
                req = req.header(k.as_str(), v.as_str());
            }
        }
        req.body(body)
            .send()
            .await
            .map_err(|e| R2Error::Network(e.to_string()))
    }

    /// Upload; returns the new ETag.
    pub async fn put(&self, key: &str, body: Vec<u8>, cond: Cond) -> Result<String, R2Error> {
        let extra = match cond {
            Cond::None => vec![],
            Cond::IfMatch(e) => vec![("if-match".to_string(), e)],
            Cond::IfNoneMatch => vec![("if-none-match".to_string(), "*".to_string())],
        };
        let r = self.send("PUT", Some(key), &[], body, &extra).await?;
        match r.status().as_u16() {
            200..=299 => Ok(r
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()),
            412 => Err(R2Error::PreconditionFailed),
            s => Err(R2Error::Status(s, r.text().await.unwrap_or_default())),
        }
    }

    /// Download; `None` if the object doesn't exist. Returns the body and ETag.
    pub async fn get(&self, key: &str) -> Result<Option<(Vec<u8>, String)>, R2Error> {
        let r = self.send("GET", Some(key), &[], vec![], &[]).await?;
        match r.status().as_u16() {
            200 => {
                let etag = r
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                let b = r.bytes().await.map_err(|e| R2Error::Network(e.to_string()))?;
                Ok(Some((b.to_vec(), etag)))
            }
            404 => Ok(None),
            s => Err(R2Error::Status(s, r.text().await.unwrap_or_default())),
        }
    }

    /// Every object under `prefix` (follows continuation tokens).
    pub async fn list(&self, prefix: &str) -> Result<Vec<Listed>, R2Error> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut q = vec![
                ("list-type".to_string(), "2".to_string()),
                ("prefix".to_string(), prefix.to_string()),
            ];
            if let Some(t) = &token {
                q.push(("continuation-token".to_string(), t.clone()));
            }
            let r = self.send("GET", None, &q, vec![], &[]).await?;
            let status = r.status().as_u16();
            let text = r.text().await.unwrap_or_default();
            if status != 200 {
                return Err(R2Error::Status(status, text));
            }
            for item in xml_blocks(&text, "Contents") {
                out.push(Listed {
                    key: xml_value(item, "Key").unwrap_or_default(),
                    last_modified: xml_value(item, "LastModified").unwrap_or_default(),
                    size: xml_value(item, "Size").and_then(|s| s.parse().ok()).unwrap_or(0),
                });
            }
            token = xml_value(&text, "NextContinuationToken");
            if xml_value(&text, "IsTruncated").as_deref() != Some("true") || token.is_none() {
                return Ok(out);
            }
        }
    }
}

fn xml_blocks<'a>(s: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(a) = rest.find(&open) {
        let after = &rest[a + open.len()..];
        let Some(b) = after.find(&close) else { break };
        out.push(&after[..b]);
        rest = &after[b + close.len()..];
    }
    out
}

fn xml_value(s: &str, tag: &str) -> Option<String> {
    xml_blocks(s, tag).first().map(|v| {
        v.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AWS documentation example: "GET Object" with a Range header (Signature V4 for S3).
    #[test]
    fn sigv4_matches_aws_example() {
        let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let auth = sign_v4(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            "us-east-1",
            "GET",
            "/test.txt",
            &[],
            &[
                ("host".into(), "examplebucket.s3.amazonaws.com".into()),
                ("range".into(), "bytes=0-9".into()),
                ("x-amz-content-sha256".into(), empty.into()),
                ("x-amz-date".into(), "20130524T000000Z".into()),
            ],
            empty,
            "20130524T000000Z",
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    /// Cross-checked with botocore's S3SigV4Auth (region "auto", 2026-11-20T10:00:00Z): a
    /// conditional PUT with an escaped key, and a ListObjectsV2 with an empty prefix.
    #[test]
    fn sigv4_matches_botocore_for_rfops_requests() {
        let h = |body: &[u8], extra: &[(&str, &str)]| {
            let mut v = vec![
                ("host".to_string(), "acct.r2.cloudflarestorage.com".to_string()),
                ("x-amz-content-sha256".to_string(), sha256_hex(body)),
                ("x-amz-date".to_string(), "20261120T100000Z".to_string()),
            ];
            v.extend(extra.iter().map(|(k, x)| (k.to_string(), x.to_string())));
            v
        };
        let put = sign_v4(
            "AK",
            "SK",
            "auto",
            "PUT",
            &uri_encode("/rfops/auto/g000001/rfops-a b.db", true),
            &[],
            &h(b"hello", &[("if-match", "\"3\"")]),
            &sha256_hex(b"hello"),
            "20261120T100000Z",
        );
        assert!(put.ends_with("SignedHeaders=host;if-match;x-amz-content-sha256;x-amz-date, Signature=1f05a935191928f17c4d0a9357f2c7aff3d196b7f3edfd31c77d6b2c9f49721b"), "{put}");
        let list = sign_v4(
            "AK",
            "SK",
            "auto",
            "GET",
            "/rfops",
            &[("list-type".into(), "2".into()), ("prefix".into(), "".into())],
            &h(b"", &[]),
            &sha256_hex(b""),
            "20261120T100000Z",
        );
        assert!(list.ends_with("Signature=76f6ea01cd24c3118849fde646b498eb6fa422743dfcf7a548c9b1d63b3732bd"), "{list}");
    }

    #[test]
    fn list_xml_parsing() {
        let x = "<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>g1/auto/a.db</Key><LastModified>2026-11-20T10:00:00Z</LastModified><Size>12</Size></Contents><Contents><Key>g2/pinned/b&amp;c.db</Key><Size>3</Size></Contents></ListBucketResult>";
        let items: Vec<_> = xml_blocks(x, "Contents")
            .into_iter()
            .map(|b| xml_value(b, "Key").unwrap())
            .collect();
        assert_eq!(items, vec!["g1/auto/a.db", "g2/pinned/b&c.db"]);
    }
}
