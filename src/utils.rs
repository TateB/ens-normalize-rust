use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsError {
    message: String,
}

impl EnsError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for EnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EnsError {}

pub type Result<T> = std::result::Result<T, EnsError>;

pub fn hex_cp(cp: u32) -> String {
    let mut s = format!("{cp:X}");
    if s.len() < 2 {
        s.insert(0, '0');
    }
    s
}

pub fn quote_cp(cp: u32) -> String {
    format!("{{{}}}", hex_cp(cp))
}

pub fn explode_cp(s: &str) -> Vec<u32> {
    s.chars().map(|c| c as u32).collect()
}

pub fn str_from_cps(cps: &[u32]) -> Result<String> {
    let mut s = String::new();
    for &cp in cps {
        let ch = char::from_u32(cp)
            .ok_or_else(|| EnsError::new(format!("invalid code point: {}", quote_cp(cp))))?;
        s.push(ch);
    }
    Ok(s)
}

pub(crate) fn compare_arrays(a: &[u32], b: &[u32]) -> i32 {
    let mut c = a.len() as i32 - b.len() as i32;
    let mut i = 0;
    while c == 0 && i < a.len() {
        c = a[i] as i32 - b[i] as i32;
        i += 1;
    }
    c
}

pub(crate) fn array_replace(v: &mut [u32], a: u32, b: u32) {
    for cp in v {
        if *cp == a {
            *cp = b;
        }
    }
}

pub(crate) fn bidi_qq(s: &str) -> String {
    format!("\"{s}\"\u{200E}")
}

pub fn safe_str_from_cps(cps: &[u32], max: Option<usize>) -> String {
    safe_str_from_cps_with(cps, max, &quote_cp)
}

pub(crate) fn safe_str_from_cps_with(
    cps: &[u32],
    max: Option<usize>,
    quoter: &dyn Fn(u32) -> String,
) -> String {
    use crate::spec::{is_combining_mark, should_escape};

    let max = max.unwrap_or(usize::MAX);
    let mut working: Vec<u32> = if cps.len() > max {
        let half = max >> 1;
        let mut v = Vec::with_capacity(half * 2 + 1);
        v.extend_from_slice(&cps[..half]);
        v.push(0x2026);
        v.extend_from_slice(&cps[cps.len().saturating_sub(half)..]);
        v
    } else {
        cps.to_vec()
    };

    let mut buf = String::new();
    if working
        .first()
        .copied()
        .is_some_and(|cp| is_combining_mark(cp, false))
    {
        buf.push('\u{25CC}');
    }

    let mut prev = 0;
    for i in 0..working.len() {
        let cp = working[i];
        if should_escape(cp) {
            if let Ok(s) = str_from_cps(&working[prev..i]) {
                buf.push_str(&s);
            }
            buf.push_str(&quoter(cp));
            prev = i + 1;
        }
    }
    if let Ok(s) = str_from_cps(&working[prev..]) {
        buf.push_str(&s);
    }
    working.clear();
    buf
}
