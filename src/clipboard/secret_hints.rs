const VALUE_SECRET_MIMETYPES: [&str; 2] =
    ["x-kde-passwordmanagerhint", "x-gnome-passwordmanagerhint"];

const PRESENCE_SECRET_MIMETYPES: [&str; 1] = ["application/x-nspasteboard-concealed-type"];

pub const SECRET_HINT_MIMETYPES: [&str; 3] = [
    "x-kde-passwordManagerHint",
    "x-gnome-passwordManagerHint",
    "application/x-nspasteboard-concealed-type",
];

fn normalized_mimetype(mimetype: &str) -> String {
    mimetype.to_lowercase()
}

pub fn is_value_secret_mimetype(mimetype: &str) -> bool {
    let normalized = normalized_mimetype(mimetype);
    VALUE_SECRET_MIMETYPES.contains(&normalized.as_str())
}

pub fn is_presence_secret_mimetype(mimetype: &str) -> bool {
    let normalized = normalized_mimetype(mimetype);
    PRESENCE_SECRET_MIMETYPES.contains(&normalized.as_str())
}

pub fn is_secret_hint_mimetype(mimetype: &str) -> bool {
    let normalized = normalized_mimetype(mimetype);
    VALUE_SECRET_MIMETYPES.contains(&normalized.as_str())
        || PRESENCE_SECRET_MIMETYPES.contains(&normalized.as_str())
}

pub fn is_strict_secret_text(text: &str) -> bool {
    text.replace('\0', "").trim() == "secret"
}

pub fn decode_secret_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn is_secret_hint_payload(mimetype: &str, bytes: &[u8]) -> bool {
    if is_presence_secret_mimetype(mimetype) {
        return true;
    }
    if !is_value_secret_mimetype(mimetype) {
        return false;
    }
    is_strict_secret_text(&decode_secret_bytes(bytes))
}
