//! Detached signature files.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, VerifyingKey};
use metaupd_metadata::root::KeyId;
use metaupd_metadata::{MetadataError, PublicKey, Role};
use serde::{Deserialize, Serialize};

use crate::hash::sha256;

pub const SIGNATURE_FORMAT: &str = "meta-signature-v1";
/// More entries than this are refused rather than processed.
const MAX_SIGNATURES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureFile {
    pub format: String,
    pub role: Role,
    pub signatures: Vec<SignatureEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureEntry {
    pub keyid: KeyId,
    /// Ed25519 signature over [`signing_message`], in standard base64.
    pub sig: String,
}

impl SignatureFile {
    pub fn new(role: Role) -> Self {
        Self {
            format: SIGNATURE_FORMAT.to_owned(),
            role,
            signatures: Vec::new(),
        }
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, MetadataError> {
        let file: Self = serde_json::from_slice(bytes).map_err(|source| MetadataError::Json {
            what: "signature file",
            source,
        })?;
        let mut problems = Vec::new();
        if file.format != SIGNATURE_FORMAT {
            problems.push(format!("unsupported format {:?}", file.format));
        }
        if file.signatures.len() > MAX_SIGNATURES {
            problems.push("too many signatures".to_owned());
        }
        if problems.is_empty() {
            Ok(file)
        } else {
            Err(MetadataError::Invalid {
                what: "signature file",
                problems,
            })
        }
    }
}

/// Identifier of a public key.
pub fn key_id(key: &PublicKey) -> KeyId {
    sha256(&key.0)
}

/// The exact bytes a key signs: a domain separator, the role, and the payload.
///
/// The payload is the document as distributed, so no canonical JSON encoding is
/// involved. NUL cannot occur in a role name, which keeps the framing unambiguous.
pub fn signing_message(role: Role, payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(payload.len() + 32);
    message.extend_from_slice(b"META-SIGNATURE-V1\0");
    message.extend_from_slice(role.as_str().as_bytes());
    message.push(0);
    message.extend_from_slice(payload);
    message
}

/// Verifies one signature, rejecting weak keys and malleable encodings.
pub(crate) fn signature_is_valid(key: &PublicKey, message: &[u8], sig: &str) -> bool {
    let Ok(raw) = BASE64.decode(sig) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&raw) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&key.0) else {
        return false;
    };
    key.verify_strict(message, &signature).is_ok()
}
