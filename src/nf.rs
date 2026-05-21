use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const S0: u32 = 0xAC00;
const L0: u32 = 0x1100;
const V0: u32 = 0x1161;
const T0: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;
const S1: u32 = S0 + S_COUNT;
const L1: u32 = L0 + L_COUNT;
const V1: u32 = V0 + V_COUNT;
const T1: u32 = T0 + T_COUNT;

#[derive(Deserialize)]
struct RawNf {
    ranks: Vec<Vec<u32>>,
    exclusions: Vec<u32>,
    decomp: Vec<(u32, Vec<u32>)>,
}

struct NfData {
    shifted_rank: HashMap<u32, u32>,
    decomp: HashMap<u32, Vec<u32>>,
    recomp: HashMap<u32, HashMap<u32, u32>>,
}

static NF: LazyLock<NfData> = LazyLock::new(|| {
    let raw: RawNf = serde_json::from_str(include_str!("../data/nf.json")).expect("valid nf.json");
    let mut shifted_rank = HashMap::new();
    for (i, cps) in raw.ranks.iter().enumerate() {
        let rank = ((i as u32) + 1) << 24;
        for &cp in cps {
            shifted_rank.insert(cp, rank);
        }
    }

    let exclusions: HashSet<u32> = raw.exclusions.into_iter().collect();
    let mut decomp = HashMap::new();
    let mut recomp: HashMap<u32, HashMap<u32, u32>> = HashMap::new();
    for (cp, mut cps) in raw.decomp {
        if !exclusions.contains(&cp) && cps.len() == 2 {
            recomp.entry(cps[0]).or_default().insert(cps[1], cp);
        }
        cps.reverse();
        decomp.insert(cp, cps);
    }

    NfData {
        shifted_rank,
        decomp,
        recomp,
    }
});

fn unpack_cc(packed: u32) -> u32 {
    (packed >> 24) & 0xFF
}

fn unpack_cp(packed: u32) -> u32 {
    packed & 0xFF_FFFF
}

fn is_hangul(cp: u32) -> bool {
    (S0..S1).contains(&cp)
}

fn compose_pair(a: u32, b: u32) -> Option<u32> {
    if (L0..L1).contains(&a) && (V0..V1).contains(&b) {
        Some(S0 + (a - L0) * N_COUNT + (b - V0) * T_COUNT)
    } else if is_hangul(a) && b > T0 && b < T1 && (a - S0).is_multiple_of(T_COUNT) {
        Some(a + (b - T0))
    } else {
        NF.recomp.get(&a).and_then(|bucket| bucket.get(&b)).copied()
    }
}

fn decomposed(cps: &[u32]) -> Vec<u32> {
    let mut ret = Vec::new();
    let mut buf = Vec::new();
    let mut check_order = false;

    let add = |ret: &mut Vec<u32>, check_order: &mut bool, cp: u32| {
        if let Some(&cc) = NF.shifted_rank.get(&cp) {
            *check_order = true;
            ret.push(cp | cc);
        } else {
            ret.push(cp);
        }
    };

    for &cp0 in cps {
        let mut cp = cp0;
        loop {
            if cp < 0x80 {
                ret.push(cp);
            } else if is_hangul(cp) {
                let s_index = cp - S0;
                let l_index = s_index / N_COUNT;
                let v_index = (s_index % N_COUNT) / T_COUNT;
                let t_index = s_index % T_COUNT;
                add(&mut ret, &mut check_order, L0 + l_index);
                add(&mut ret, &mut check_order, V0 + v_index);
                if t_index > 0 {
                    add(&mut ret, &mut check_order, T0 + t_index);
                }
            } else if let Some(mapped) = NF.decomp.get(&cp) {
                buf.extend_from_slice(mapped);
            } else {
                add(&mut ret, &mut check_order, cp);
            }

            if let Some(next) = buf.pop() {
                cp = next;
            } else {
                break;
            }
        }
    }

    if check_order && ret.len() > 1 {
        let mut prev_cc = unpack_cc(ret[0]);
        let mut i = 1;
        while i < ret.len() {
            let cc = unpack_cc(ret[i]);
            if cc == 0 || prev_cc <= cc {
                prev_cc = cc;
                i += 1;
                continue;
            }
            let mut j = i - 1;
            loop {
                ret.swap(j + 1, j);
                if j == 0 {
                    break;
                }
                j -= 1;
                prev_cc = unpack_cc(ret[j]);
                if prev_cc <= cc {
                    break;
                }
            }
            prev_cc = unpack_cc(ret[i]);
            i += 1;
        }
    }

    ret
}

fn composed_from_decomposed(v: &[u32]) -> Vec<u32> {
    let mut ret = Vec::new();
    let mut stack = Vec::new();
    let mut prev_cp: Option<u32> = None;
    let mut prev_cc = 0;

    for &packed in v {
        let cc = unpack_cc(packed);
        let cp = unpack_cp(packed);
        if let Some(prev) = prev_cp {
            if prev_cc > 0 && prev_cc >= cc {
                if cc == 0 {
                    ret.push(prev);
                    ret.append(&mut stack);
                    prev_cp = Some(cp);
                } else {
                    stack.push(cp);
                }
                prev_cc = cc;
            } else if let Some(composed) = compose_pair(prev, cp) {
                prev_cp = Some(composed);
            } else if prev_cc == 0 && cc == 0 {
                ret.push(prev);
                prev_cp = Some(cp);
            } else {
                stack.push(cp);
                prev_cc = cc;
            }
        } else if cc == 0 {
            prev_cp = Some(cp);
        } else {
            ret.push(cp);
        }
    }

    if let Some(prev) = prev_cp {
        ret.push(prev);
        ret.append(&mut stack);
    }

    ret
}

pub fn nfd(cps: &[u32]) -> Vec<u32> {
    decomposed(cps).into_iter().map(unpack_cp).collect()
}

pub fn nfc(cps: &[u32]) -> Vec<u32> {
    composed_from_decomposed(&decomposed(cps))
}
