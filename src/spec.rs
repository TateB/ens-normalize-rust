use crate::intmap::{IntMap, IntSet};
use crate::nf::{nfc, nfd};
use crate::utils::{
    EnsError, Result, array_replace, bidi_qq, compare_arrays, explode_cp, quote_cp,
    safe_str_from_cps, str_from_cps,
};
use serde::Deserialize;
use std::borrow::Cow;
use std::sync::LazyLock;

const HYPHEN: u32 = 0x2D;
const STOP: u32 = 0x2E;
const FE0F: u32 = 0xFE0F;
const UNIQUE_PH: usize = usize::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub input: Vec<u32>,
    pub offset: usize,
    pub error: Option<EnsError>,
    pub tokens: Option<Vec<Vec<u32>>>,
    pub output: Option<Vec<u32>>,
    pub emoji: Option<bool>,
    pub label_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Stop {
        cp: u32,
    },
    Disallowed {
        cp: u32,
    },
    Ignored {
        cp: u32,
    },
    Valid {
        cps: Vec<u32>,
    },
    Mapped {
        cp: u32,
        cps: Vec<u32>,
    },
    Emoji {
        input: Vec<u32>,
        cps: Vec<u32>,
        emoji: Vec<u32>,
    },
    Nfc {
        input: Vec<u32>,
        tokens0: Vec<Token>,
        cps: Vec<u32>,
        tokens: Vec<Token>,
    },
}

impl Token {
    pub fn token_type(&self) -> &'static str {
        match self {
            Token::Stop { .. } => "stop",
            Token::Disallowed { .. } => "disallowed",
            Token::Ignored { .. } => "ignored",
            Token::Valid { .. } => "valid",
            Token::Mapped { .. } => "mapped",
            Token::Emoji { .. } => "emoji",
            Token::Nfc { .. } => "nfc",
        }
    }

    pub fn cps(&self) -> Option<&[u32]> {
        match self {
            Token::Valid { cps }
            | Token::Mapped { cps, .. }
            | Token::Emoji { cps, .. }
            | Token::Nfc { cps, .. } => Some(cps),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenizeOptions {
    pub nf: bool,
}

impl Default for TokenizeOptions {
    fn default() -> Self {
        Self { nf: true }
    }
}

#[derive(Deserialize)]
struct RawSpec {
    emoji: Vec<Vec<u32>>,
    ignored: Vec<u32>,
    mapped: Vec<(u32, Vec<u32>)>,
    fenced: Vec<(u32, String)>,
    wholes: Vec<RawWhole>,
    cm: Vec<u32>,
    nsm: Vec<u32>,
    nsm_max: usize,
    escape: Vec<u32>,
    groups: Vec<RawGroup>,
    nfc_check: Vec<u32>,
}

#[derive(Deserialize)]
struct RawWhole {
    valid: Vec<u32>,
    confused: Vec<u32>,
}

#[derive(Deserialize)]
struct RawGroup {
    name: String,
    #[serde(default)]
    restricted: bool,
    primary: Vec<u32>,
    secondary: Vec<u32>,
    cm: Option<Vec<serde_json::Value>>,
}

struct Group {
    name: String,
    primary: IntSet<u32>,
    secondary: IntSet<u32>,
    check_nsm: bool,
}

impl Group {
    fn has_cp(&self, cp: u32) -> bool {
        self.primary.contains(&cp) || self.secondary.contains(&cp)
    }
}

struct Whole {
    complements: IntMap<u32, Vec<usize>>,
}

#[derive(Default)]
struct EmojiNode {
    children: IntMap<u32, usize>,
    value: Option<Vec<u32>>,
}

#[derive(Default)]
struct EmojiTrie {
    nodes: Vec<EmojiNode>,
}

impl EmojiTrie {
    fn new() -> Self {
        Self {
            nodes: vec![EmojiNode::default()],
        }
    }

    fn child_or_insert(&mut self, node: usize, cp: u32) -> usize {
        if let Some(&child) = self.nodes[node].children.get(&cp) {
            return child;
        }
        let child = self.nodes.len();
        self.nodes.push(EmojiNode::default());
        self.nodes[node].children.insert(cp, child);
        child
    }
}

struct EnsData {
    mapped: IntMap<u32, Vec<u32>>,
    ignored: IntSet<u32>,
    cm: IntSet<u32>,
    nsm: IntSet<u32>,
    nsm_check: IntSet<u32>,
    nsm_max: usize,
    escape: IntSet<u32>,
    nfc_check: IntSet<u32>,
    fenced: IntMap<u32, String>,
    groups: Vec<Group>,
    group_members: IntMap<u32, Vec<usize>>,
    primary_group: IntMap<u32, usize>,
    whole_map: IntMap<u32, usize>,
    wholes: Vec<Whole>,
    valid: IntSet<u32>,
    emoji_list: Vec<Vec<u32>>,
    emoji_root: EmojiTrie,
}

static ENS: LazyLock<EnsData> = LazyLock::new(|| {
    let raw: RawSpec =
        serde_json::from_str(include_str!("../data/spec.json")).expect("valid spec.json");
    EnsData::from_raw(raw)
});

impl EnsData {
    fn from_raw(raw: RawSpec) -> Self {
        let groups: Vec<Group> = raw
            .groups
            .into_iter()
            .map(|g| {
                let name = if g.restricted {
                    format!("Restricted[{}]", g.name)
                } else {
                    g.name
                };
                Group {
                    name,
                    primary: g.primary.into_iter().collect(),
                    secondary: g.secondary.into_iter().collect(),
                    check_nsm: g.cm.is_none(),
                }
            })
            .collect();

        let mut group_members: IntMap<u32, Vec<usize>> = IntMap::default();
        let mut primary_group = IntMap::default();
        for (i, group) in groups.iter().enumerate() {
            for &cp in &group.primary {
                primary_group.entry(cp).or_insert(i);
                let members = group_members.entry(cp).or_default();
                if !members.contains(&i) {
                    members.push(i);
                }
            }
            for &cp in &group.secondary {
                let members = group_members.entry(cp).or_default();
                if !members.contains(&i) {
                    members.push(i);
                }
            }
        }

        let mut wholes = Vec::new();
        let mut whole_map = IntMap::default();
        for raw_whole in raw.wholes {
            if raw_whole.confused.is_empty() {
                continue;
            }

            let values: Vec<u32> = raw_whole
                .valid
                .iter()
                .chain(raw_whole.confused.iter())
                .copied()
                .collect();
            let complements = compute_whole_complements(&groups, &values);
            let whole_index = wholes.len();
            for cp in raw_whole.confused {
                whole_map.insert(cp, whole_index);
            }
            wholes.push(Whole { complements });
        }

        let mut valid = IntSet::default();
        let mut multi = IntSet::default();
        for g in &groups {
            for &cp in g.primary.iter().chain(g.secondary.iter()) {
                if !valid.insert(cp) {
                    multi.insert(cp);
                }
            }
        }

        for &cp in &valid {
            if !whole_map.contains_key(&cp) && !multi.contains(&cp) {
                whole_map.insert(cp, UNIQUE_PH);
            }
        }

        let valid_vec: Vec<u32> = valid.iter().copied().collect();
        for cp in nfd(&valid_vec) {
            valid.insert(cp);
        }
        let nsm: IntSet<u32> = raw.nsm.into_iter().collect();
        let nsm_check: IntSet<u32> = valid
            .iter()
            .copied()
            .filter(|&cp| nfd(&[cp]).iter().any(|part| nsm.contains(part)))
            .collect();

        let mut emoji_list = raw.emoji;
        emoji_list.sort_by(|a, b| compare_arrays(a, b).cmp(&0));
        let mut emoji_root = EmojiTrie::new();
        for cps in &emoji_list {
            let mut prev = vec![0usize];
            for &cp in cps {
                let next: Vec<usize> = prev
                    .iter()
                    .map(|&node| emoji_root.child_or_insert(node, cp))
                    .collect();
                if cp == FE0F {
                    prev.extend(next);
                } else {
                    prev = next;
                }
            }
            for node in prev {
                emoji_root.nodes[node].value = Some(cps.clone());
            }
        }

        Self {
            mapped: raw.mapped.into_iter().collect(),
            ignored: raw.ignored.into_iter().collect(),
            cm: raw.cm.into_iter().collect(),
            nsm,
            nsm_check,
            nsm_max: raw.nsm_max,
            escape: raw.escape.into_iter().collect(),
            nfc_check: raw.nfc_check.into_iter().collect(),
            fenced: raw.fenced.into_iter().collect(),
            groups,
            group_members,
            primary_group,
            whole_map,
            wholes,
            valid,
            emoji_list,
            emoji_root,
        }
    }
}

struct WholeRec {
    groups: Vec<usize>,
    values: Vec<u32>,
}

fn push_unique(v: &mut Vec<usize>, x: usize) {
    if !v.contains(&x) {
        v.push(x);
    }
}

fn compute_whole_complements(groups: &[Group], values: &[u32]) -> IntMap<u32, Vec<usize>> {
    let mut recs: Vec<WholeRec> = Vec::new();
    for &cp in values {
        let gs: Vec<usize> = groups
            .iter()
            .enumerate()
            .filter_map(|(i, g)| g.has_cp(cp).then_some(i))
            .collect();
        let rec_index = recs
            .iter()
            .position(|rec| gs.iter().any(|g| rec.groups.contains(g)));
        let rec_index = match rec_index {
            Some(i) => i,
            None => {
                recs.push(WholeRec {
                    groups: Vec::new(),
                    values: Vec::new(),
                });
                recs.len() - 1
            }
        };
        recs[rec_index].values.push(cp);
        for g in gs {
            push_unique(&mut recs[rec_index].groups, g);
        }
    }

    let mut union = Vec::new();
    for rec in &recs {
        for &g in &rec.groups {
            push_unique(&mut union, g);
        }
    }

    let mut complements = IntMap::default();
    for rec in recs {
        let complement: Vec<usize> = union
            .iter()
            .copied()
            .filter(|g| !rec.groups.contains(g))
            .collect();
        for cp in rec.values {
            complements.insert(cp, complement.clone());
        }
    }
    complements
}

#[derive(Clone)]
struct NormToken {
    cps: Vec<u32>,
    is_emoji: bool,
}

pub fn is_combining_mark(cp: u32, only_nsm: bool) -> bool {
    if only_nsm {
        ENS.nsm.contains(&cp)
    } else {
        ENS.cm.contains(&cp)
    }
}

pub fn should_escape(cp: u32) -> bool {
    ENS.escape.contains(&cp)
}

pub fn ens_emoji() -> Vec<Vec<u32>> {
    ENS.emoji_list.clone()
}

pub fn ens_normalize_fragment(frag: &str, decompose: bool) -> Result<String> {
    let nf = if decompose {
        NormalizeForm::Nfd
    } else {
        NormalizeForm::Nfc
    };
    let mut out = Vec::new();
    for (i, label) in frag.split('.').enumerate() {
        if i > 0 {
            out.push(STOP);
        }
        let input = explode_cp(label);
        let tokens = tokens_from_str(&input, nf, EmojiFilter::DropFe0f)?;
        out.extend(tokens.into_iter().flat_map(|t| t.cps));
    }
    str_from_cps(&out)
}

pub fn ens_normalize(name: &str) -> Result<String> {
    if let Some(result) = normalize_ascii(name) {
        return result;
    }
    normalize_labels(name)
}

pub fn ens_beautify(name: &str) -> Result<String> {
    let mut labels = split(name, NormalizeForm::Nfc, EmojiFilter::Preserve);
    for label in &mut labels {
        if label.error.is_some() {
            break;
        }
        if label.label_type.as_deref() != Some("Greek")
            && let Some(output) = &mut label.output
        {
            array_replace(output, 0x3BE, 0x39E);
        }
    }
    flatten(labels)
}

pub fn ens_split(name: &str, preserve_emoji: bool) -> Vec<Label> {
    split(
        name,
        NormalizeForm::Nfc,
        if preserve_emoji {
            EmojiFilter::Preserve
        } else {
            EmojiFilter::DropFe0f
        },
    )
}

fn split(name: &str, nf: NormalizeForm, ef: EmojiFilter) -> Vec<Label> {
    if name.is_empty() {
        return Vec::new();
    }

    let mut offset = 0usize;
    name.split('.')
        .map(|label| {
            let input = explode_cp(label);
            let mut info = Label {
                input: input.clone(),
                offset,
                error: None,
                tokens: None,
                output: None,
                emoji: None,
                label_type: None,
            };
            offset += input.len() + 1;

            if let Err(err) = process_label(&input, nf, ef, &mut info) {
                info.error = Some(err);
            }
            info
        })
        .collect()
}

fn process_label(
    input: &[u32],
    nf: NormalizeForm,
    ef: EmojiFilter,
    info: &mut Label,
) -> Result<()> {
    let tokens = tokens_from_str(input, nf, ef)?;
    info.tokens = Some(tokens.iter().map(|t| t.cps.clone()).collect());
    if tokens.is_empty() {
        return Err(EnsError::new("empty label"));
    }

    let output: Vec<u32> = tokens.iter().flat_map(|t| t.cps.iter().copied()).collect();
    info.output = Some(output.clone());
    check_leading_underscore(&output)?;
    let emoji = tokens.len() > 1 || tokens[0].is_emoji;
    info.emoji = Some(emoji);
    let label_type = if !emoji && output.iter().all(|&cp| cp < 0x80) {
        check_label_extension(&output)?;
        "ASCII".to_string()
    } else {
        let chars_storage;
        let chars: &[u32] = if emoji {
            chars_storage = tokens
                .iter()
                .filter(|t| !t.is_emoji)
                .flat_map(|t| t.cps.iter().copied())
                .collect::<Vec<_>>();
            &chars_storage
        } else {
            &output
        };
        if chars.is_empty() {
            "Emoji".to_string()
        } else {
            if ENS.cm.contains(&output[0]) {
                return Err(error_placement("leading combining mark"));
            }
            for i in 1..tokens.len() {
                if !tokens[i].is_emoji && ENS.cm.contains(&tokens[i].cps[0]) {
                    let prev = str_from_cps(&tokens[i - 1].cps)?;
                    let mark = safe_str_from_cps(&[tokens[i].cps[0]], None);
                    return Err(error_placement(&format!(
                        "emoji + combining mark: \"{prev} + {mark}\""
                    )));
                }
            }

            check_fenced(&output)?;
            let unique = unique_preserving_order(chars);
            let group = determine_group(&unique)?;
            check_group(group, chars)?;
            check_whole(group, &unique)?;
            ENS.groups[group].name.clone()
        }
    };

    info.label_type = Some(label_type);
    Ok(())
}

fn process_label_output(input: &[u32], nf: NormalizeForm, ef: EmojiFilter) -> Result<Vec<u32>> {
    let tokens = tokens_from_str(input, nf, ef)?;
    if tokens.is_empty() {
        return Err(EnsError::new("empty label"));
    }

    let output: Vec<u32> = tokens.iter().flat_map(|t| t.cps.iter().copied()).collect();
    check_leading_underscore(&output)?;
    let emoji = tokens.len() > 1 || tokens[0].is_emoji;
    if !emoji && output.iter().all(|&cp| cp < 0x80) {
        check_label_extension(&output)?;
    } else {
        let chars_storage;
        let chars: &[u32] = if emoji {
            chars_storage = tokens
                .iter()
                .filter(|t| !t.is_emoji)
                .flat_map(|t| t.cps.iter().copied())
                .collect::<Vec<_>>();
            &chars_storage
        } else {
            &output
        };
        if !chars.is_empty() {
            if ENS.cm.contains(&output[0]) {
                return Err(error_placement("leading combining mark"));
            }
            for i in 1..tokens.len() {
                if !tokens[i].is_emoji && ENS.cm.contains(&tokens[i].cps[0]) {
                    let prev = str_from_cps(&tokens[i - 1].cps)?;
                    let mark = safe_str_from_cps(&[tokens[i].cps[0]], None);
                    return Err(error_placement(&format!(
                        "emoji + combining mark: \"{prev} + {mark}\""
                    )));
                }
            }

            check_fenced(&output)?;
            let unique = unique_preserving_order(chars);
            let group = determine_group(&unique)?;
            check_group(group, chars)?;
            check_whole(group, &unique)?;
        }
    }

    Ok(output)
}

fn process_text_label_output(input: &[u32]) -> Option<Result<Vec<u32>>> {
    let mut chars = Vec::with_capacity(input.len());
    for &cp in input {
        if ENS.emoji_root.nodes[0].children.contains_key(&cp) {
            return None;
        }
        if ENS.valid.contains(&cp) {
            chars.push(cp);
        } else if let Some(cps) = ENS.mapped.get(&cp) {
            chars.extend_from_slice(cps);
        } else if !ENS.ignored.contains(&cp) {
            return Some(Err(error_disallowed(cp)));
        }
    }

    let output = NormalizeForm::Nfc.apply(&chars);
    Some(validate_text_label_output(&output).map(|()| output))
}

fn validate_text_label_output(output: &[u32]) -> Result<()> {
    if output.is_empty() {
        return Err(EnsError::new("empty label"));
    }
    check_leading_underscore(output)?;
    if output.iter().all(|&cp| cp < 0x80) {
        check_label_extension(output)?;
    } else {
        if ENS.cm.contains(&output[0]) {
            return Err(error_placement("leading combining mark"));
        }
        check_fenced(output)?;
        let unique = unique_preserving_order(output);
        let group = determine_group(&unique)?;
        check_group(group, output)?;
        check_whole(group, &unique)?;
    }
    Ok(())
}

fn normalize_labels(name: &str) -> Result<String> {
    if name.is_empty() {
        return Ok(String::new());
    }

    let labels: Vec<&str> = name.split('.').collect();
    let multiple = labels.len() != 1;
    let mut out = String::with_capacity(name.len());
    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        if let Some(label) = normalize_ascii_label(label) {
            out.push_str(&label);
            continue;
        }
        let input = explode_cp(label);
        let result = process_text_label_output(&input).unwrap_or_else(|| {
            process_label_output(&input, NormalizeForm::Nfc, EmojiFilter::DropFe0f)
        });
        match result {
            Ok(output) => out.push_str(&str_from_cps(&output)?),
            Err(error) if multiple => {
                let safe = safe_str_from_cps(&input, Some(63));
                return Err(EnsError::new(format!(
                    "Invalid label {}: {}",
                    bidi_qq(&safe),
                    error.message()
                )));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(out)
}

fn normalize_ascii(name: &str) -> Option<Result<String>> {
    if name.is_empty() {
        return Some(Ok(String::new()));
    }
    if !name.is_ascii() {
        return None;
    }

    let mut start = 0;
    let mut changed = false;
    for (i, byte) in name.bytes().enumerate() {
        if byte == b'.' {
            if !valid_ascii_label(&name.as_bytes()[start..i]) {
                return None;
            }
            start = i + 1;
        } else if byte.is_ascii_uppercase() {
            changed = true;
        } else if !is_valid_ascii_byte(byte) {
            return None;
        }
    }

    if !valid_ascii_label(&name.as_bytes()[start..]) {
        return None;
    }

    if changed {
        let mut out = String::with_capacity(name.len());
        for byte in name.bytes() {
            if byte.is_ascii_uppercase() {
                out.push(char::from(byte + 32));
            } else {
                out.push(char::from(byte));
            }
        }
        Some(Ok(out))
    } else {
        Some(Ok(name.to_owned()))
    }
}

fn normalize_ascii_label(label: &str) -> Option<Cow<'_, str>> {
    if label.is_empty() || !label.is_ascii() {
        return None;
    }
    let bytes = label.as_bytes();
    if !valid_ascii_label(bytes) {
        return None;
    }

    let mut changed = false;
    for &byte in bytes {
        if byte.is_ascii_uppercase() {
            changed = true;
        } else if !is_valid_ascii_byte(byte) {
            return None;
        }
    }

    if changed {
        let mut out = String::with_capacity(label.len());
        for byte in label.bytes() {
            if byte.is_ascii_uppercase() {
                out.push(char::from(byte + 32));
            } else {
                out.push(char::from(byte));
            }
        }
        Some(Cow::Owned(out))
    } else {
        Some(Cow::Borrowed(label))
    }
}

fn is_valid_ascii_byte(byte: u8) -> bool {
    matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'$')
}

fn valid_ascii_label(label: &[u8]) -> bool {
    if label.is_empty() {
        return false;
    }
    if label.len() >= 4 && label[2] == b'-' && label[3] == b'-' {
        return false;
    }
    match label.iter().rposition(|&cp| cp == b'_') {
        Some(0) | None => true,
        Some(pos) => label[..pos].iter().all(|&cp| cp == b'_'),
    }
}

fn unique_preserving_order(cps: &[u32]) -> Vec<u32> {
    if cps.len() <= 64 {
        let mut unique = Vec::new();
        for &cp in cps {
            if !unique.contains(&cp) {
                unique.push(cp);
            }
        }
        return unique;
    }

    let mut seen = IntSet::default();
    let mut unique = Vec::new();
    for &cp in cps {
        if seen.insert(cp) {
            unique.push(cp);
        }
    }
    unique
}

fn check_label_extension(cps: &[u32]) -> Result<()> {
    if cps.len() >= 4 && cps[2] == HYPHEN && cps[3] == HYPHEN {
        let s = str_from_cps(&cps[..4])?;
        Err(EnsError::new(format!("invalid label extension: \"{s}\"")))
    } else {
        Ok(())
    }
}

fn check_leading_underscore(cps: &[u32]) -> Result<()> {
    const UNDERSCORE: u32 = 0x5F;
    if let Some(mut i) = cps.iter().rposition(|&cp| cp == UNDERSCORE) {
        while i > 0 {
            i -= 1;
            if cps[i] != UNDERSCORE {
                return Err(EnsError::new("underscore allowed only at start"));
            }
        }
    }
    Ok(())
}

fn check_fenced(cps: &[u32]) -> Result<()> {
    if cps.is_empty() {
        return Ok(());
    }
    let mut prev = ENS.fenced.get(&cps[0]);
    if let Some(prev) = prev {
        return Err(error_placement(&format!("leading {prev}")));
    }

    let mut last = usize::MAX;
    for (i, &cp) in cps.iter().enumerate().skip(1) {
        if let Some(matched) = ENS.fenced.get(&cp) {
            if last == i {
                return Err(error_placement(&format!("{} + {matched}", prev.unwrap())));
            }
            last = i + 1;
            prev = Some(matched);
        }
    }
    if last == cps.len()
        && let Some(prev) = prev
    {
        return Err(error_placement(&format!("trailing {prev}")));
    }
    Ok(())
}

fn determine_group(unique: &[u32]) -> Result<usize> {
    let mut groups: Option<Vec<usize>> = None;
    for &cp in unique {
        let Some(cp_groups) = ENS.group_members.get(&cp) else {
            return Err(error_disallowed(cp));
        };
        let gs: Vec<usize> = if let Some(groups) = groups.take() {
            let first = groups[0];
            let filtered: Vec<usize> = groups
                .into_iter()
                .filter(|i| cp_groups.contains(i))
                .collect();
            if filtered.is_empty() {
                return Err(error_group_member(first, cp));
            }
            filtered
        } else {
            cp_groups.clone()
        };
        if gs.len() == 1 {
            return Ok(gs[0]);
        }
        groups = Some(gs);
    }
    Ok(groups.expect("unique has at least one code point")[0])
}

fn check_group(group: usize, cps: &[u32]) -> Result<()> {
    let g = &ENS.groups[group];
    for &cp in cps {
        if !g.has_cp(cp) {
            return Err(error_group_member(group, cp));
        }
    }

    if g.check_nsm && cps.iter().any(|cp| ENS.nsm_check.contains(cp)) {
        let decomposed = nfd(cps);
        let mut i = 1usize;
        while i < decomposed.len() {
            if ENS.nsm.contains(&decomposed[i]) {
                let mut j = i + 1;
                while j < decomposed.len() && ENS.nsm.contains(&decomposed[j]) {
                    for k in i..j {
                        if decomposed[k] == decomposed[j] {
                            return Err(EnsError::new(format!(
                                "duplicate non-spacing marks: {}",
                                quoted_cp(decomposed[j])
                            )));
                        }
                    }
                    j += 1;
                }
                if j - i > ENS.nsm_max {
                    let s = safe_str_from_cps(&decomposed[i - 1..j], None);
                    return Err(EnsError::new(format!(
                        "excessive non-spacing marks: {} ({}/{})",
                        bidi_qq(&s),
                        j - i,
                        ENS.nsm_max
                    )));
                }
                i = j;
            } else {
                i += 1;
            }
        }
    }

    Ok(())
}

fn check_whole(group: usize, unique: &[u32]) -> Result<()> {
    let mut maker: Option<Vec<usize>> = None;
    let mut shared = Vec::new();
    for &cp in unique {
        match ENS.whole_map.get(&cp).copied() {
            Some(UNIQUE_PH) => return Ok(()),
            Some(whole_index) => {
                let set = ENS.wholes[whole_index]
                    .complements
                    .get(&cp)
                    .cloned()
                    .unwrap_or_default();
                maker = Some(match maker {
                    Some(prev) => prev.into_iter().filter(|g| set.contains(g)).collect(),
                    None => set,
                });
                if maker.as_ref().is_some_and(|m| m.is_empty()) {
                    return Ok(());
                }
            }
            None => shared.push(cp),
        }
    }

    if let Some(maker) = maker {
        for other in maker {
            if shared.iter().all(|&cp| ENS.groups[other].has_cp(cp)) {
                return Err(EnsError::new(format!(
                    "whole-script confusable: {}/{}",
                    ENS.groups[group].name, ENS.groups[other].name
                )));
            }
        }
    }
    Ok(())
}

fn flatten(labels: Vec<Label>) -> Result<String> {
    let multiple = labels.len() != 1;
    let mut out = Vec::new();
    for label in labels {
        if let Some(error) = label.error {
            if multiple {
                let safe = safe_str_from_cps(&label.input, Some(63));
                return Err(EnsError::new(format!(
                    "Invalid label {}: {}",
                    bidi_qq(&safe),
                    error.message()
                )));
            }
            return Err(error);
        }
        out.push(str_from_cps(label.output.as_deref().unwrap_or_default())?);
    }
    Ok(out.join("."))
}

fn quoted_cp(cp: u32) -> String {
    let prefix = if should_escape(cp) {
        String::new()
    } else {
        format!("{} ", bidi_qq(&safe_str_from_cps(&[cp], None)))
    };
    format!("{prefix}{}", quote_cp(cp))
}

fn error_disallowed(cp: u32) -> EnsError {
    EnsError::new(format!("disallowed character: {}", quoted_cp(cp)))
}

fn error_group_member(group: usize, cp: u32) -> EnsError {
    let mut quoted = quoted_cp(cp);
    if let Some(&gg) = ENS.primary_group.get(&cp) {
        let gg = &ENS.groups[gg];
        quoted = format!("{} {quoted}", gg.name);
    }
    EnsError::new(format!(
        "illegal mixture: {} + {quoted}",
        ENS.groups[group].name
    ))
}

fn error_placement(where_: &str) -> EnsError {
    EnsError::new(format!("illegal placement: {where_}"))
}

#[derive(Debug, Clone, Copy)]
enum NormalizeForm {
    Nfc,
    Nfd,
}

impl NormalizeForm {
    fn apply(self, cps: &[u32]) -> Vec<u32> {
        match self {
            Self::Nfc if !requires_check(cps) => cps.to_vec(),
            Self::Nfc => nfc(cps),
            Self::Nfd => nfd(cps),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum EmojiFilter {
    Preserve,
    DropFe0f,
}

fn filter_emoji(cps: &[u32], filter: EmojiFilter) -> Vec<u32> {
    match filter {
        EmojiFilter::Preserve => cps.to_vec(),
        EmojiFilter::DropFe0f => cps.iter().copied().filter(|&cp| cp != FE0F).collect(),
    }
}

fn tokens_from_str(input: &[u32], nf: NormalizeForm, ef: EmojiFilter) -> Result<Vec<NormToken>> {
    let mut ret = Vec::new();
    let mut chars = Vec::new();
    let mut input = input.to_vec();
    input.reverse();

    while !input.is_empty() {
        if let Some(emoji) = consume_emoji_reversed(&mut input, None) {
            if !chars.is_empty() {
                ret.push(NormToken {
                    cps: nf.apply(&chars),
                    is_emoji: false,
                });
                chars.clear();
            }
            ret.push(NormToken {
                cps: filter_emoji(&emoji, ef),
                is_emoji: true,
            });
        } else {
            let cp = input.pop().expect("input is not empty");
            if ENS.valid.contains(&cp) {
                chars.push(cp);
            } else if let Some(cps) = ENS.mapped.get(&cp) {
                chars.extend_from_slice(cps);
            } else if !ENS.ignored.contains(&cp) {
                return Err(error_disallowed(cp));
            }
        }
    }

    if !chars.is_empty() {
        ret.push(NormToken {
            cps: nf.apply(&chars),
            is_emoji: false,
        });
    }

    Ok(ret)
}

fn consume_emoji_reversed(input: &mut Vec<u32>, eaten: Option<&mut Vec<u32>>) -> Option<Vec<u32>> {
    let mut eaten = eaten;
    let mut node = 0usize;
    let mut emoji = None;
    let mut pos = input.len();
    while pos > 0 {
        pos -= 1;
        let cp = input[pos];
        let Some(&child) = ENS.emoji_root.nodes[node].children.get(&cp) else {
            break;
        };
        node = child;
        if let Some(value) = ENS.emoji_root.nodes[node].value.clone() {
            if let Some(eaten) = eaten.as_deref_mut() {
                eaten.extend(input[pos..].iter().rev().copied());
            }
            input.truncate(pos);
            emoji = Some(value);
        }
    }
    emoji
}

pub fn ens_tokenize(name: &str) -> Vec<Token> {
    ens_tokenize_with_options(name, TokenizeOptions::default())
}

pub fn ens_tokenize_with_options(name: &str, options: TokenizeOptions) -> Vec<Token> {
    tokenize(name, options.nf)
}

fn tokenize(name: &str, nf: bool) -> Vec<Token> {
    let mut input = explode_cp(name);
    input.reverse();
    let mut eaten = Vec::new();
    let mut tokens = Vec::new();

    while !input.is_empty() {
        if let Some(emoji) = consume_emoji_reversed(&mut input, Some(&mut eaten)) {
            tokens.push(Token::Emoji {
                input: std::mem::take(&mut eaten),
                cps: filter_emoji(&emoji, EmojiFilter::DropFe0f),
                emoji,
            });
        } else {
            let cp = input.pop().expect("input is not empty");
            if cp == STOP {
                tokens.push(Token::Stop { cp });
            } else if ENS.valid.contains(&cp) {
                tokens.push(Token::Valid { cps: vec![cp] });
            } else if ENS.ignored.contains(&cp) {
                tokens.push(Token::Ignored { cp });
            } else if let Some(cps) = ENS.mapped.get(&cp) {
                tokens.push(Token::Mapped {
                    cp,
                    cps: cps.clone(),
                });
            } else {
                tokens.push(Token::Disallowed { cp });
            }
        }
    }

    if nf {
        apply_token_nfc(&mut tokens);
    }

    collapse_valid_tokens(tokens)
}

fn is_valid_or_mapped(token: &Token) -> bool {
    matches!(token, Token::Valid { .. } | Token::Mapped { .. })
}

fn valid_or_mapped_cps(token: &Token) -> Option<&[u32]> {
    match token {
        Token::Valid { cps } | Token::Mapped { cps, .. } => Some(cps),
        _ => None,
    }
}

fn requires_check(cps: &[u32]) -> bool {
    cps.iter().any(|cp| ENS.nfc_check.contains(cp))
}

fn apply_token_nfc(tokens: &mut Vec<Token>) {
    let mut i = 0usize;
    let mut start: Option<usize> = None;
    while i < tokens.len() {
        if is_valid_or_mapped(&tokens[i]) {
            let cps = valid_or_mapped_cps(&tokens[i]).unwrap();
            if requires_check(cps) {
                let mut end = i + 1;
                let mut pos = end;
                while pos < tokens.len() {
                    if let Some(cps) = valid_or_mapped_cps(&tokens[pos]) {
                        if !requires_check(cps) {
                            break;
                        }
                        end = pos + 1;
                    } else if !matches!(tokens[pos], Token::Ignored { .. }) {
                        break;
                    }
                    pos += 1;
                }
                let start_i = start.unwrap_or(i);
                let slice = tokens[start_i..end].to_vec();
                let cps0: Vec<u32> = slice
                    .iter()
                    .filter_map(valid_or_mapped_cps)
                    .flat_map(|cps| cps.iter().copied())
                    .collect();
                let cps = nfc(&cps0);
                if compare_arrays(&cps, &cps0) != 0 {
                    let text = str_from_cps(&cps).unwrap_or_default();
                    let replacement = Token::Nfc {
                        input: cps0,
                        tokens0: collapse_valid_tokens(slice),
                        cps,
                        tokens: tokenize(&text, false),
                    };
                    tokens.splice(start_i..end, [replacement]);
                    i = start_i;
                } else {
                    i = end.saturating_sub(1);
                }
                start = None;
            } else {
                start = Some(i);
            }
        } else if !matches!(tokens[i], Token::Ignored { .. }) {
            start = None;
        }
        i += 1;
    }
}

fn collapse_valid_tokens(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        if let Token::Valid { .. } = &tokens[i] {
            let mut cps = Vec::new();
            while i < tokens.len() {
                if let Token::Valid { cps: next } = &tokens[i] {
                    cps.extend_from_slice(next);
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Token::Valid { cps });
        } else {
            out.push(tokens[i].clone());
            i += 1;
        }
    }
    out
}
