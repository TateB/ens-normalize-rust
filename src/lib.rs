mod intmap;
mod nf;
mod spec;
mod utils;

pub use nf::{nfc, nfd};
pub use spec::{
    Label, Token, TokenizeOptions, ens_beautify, ens_emoji, ens_normalize, ens_normalize_fragment,
    ens_split, ens_tokenize, ens_tokenize_with_options, is_combining_mark, should_escape,
};
pub use utils::{EnsError, Result, explode_cp, quote_cp, safe_str_from_cps, str_from_cps};

pub fn normalize(name: &str) -> Result<String> {
    ens_normalize(name)
}

#[cfg(feature = "wasm")]
mod wasm {
    use crate::{EnsError, Token};
    use serde_json::{Value, json};
    use wasm_bindgen::prelude::*;

    fn js_error(err: EnsError) -> JsValue {
        JsValue::from_str(err.message())
    }

    fn to_js<T: serde::Serialize>(value: &T) -> std::result::Result<JsValue, JsValue> {
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        value
            .serialize(&serializer)
            .map_err(|err| JsValue::from_str(&err.to_string()))
    }

    fn token_to_value(token: &Token) -> Value {
        match token {
            Token::Stop { cp } => json!({ "type": "stop", "cp": cp }),
            Token::Disallowed { cp } => json!({ "type": "disallowed", "cp": cp }),
            Token::Ignored { cp } => json!({ "type": "ignored", "cp": cp }),
            Token::Valid { cps } => json!({ "type": "valid", "cps": cps }),
            Token::Mapped { cp, cps } => json!({ "type": "mapped", "cp": cp, "cps": cps }),
            Token::Emoji { input, cps, emoji } => {
                json!({ "type": "emoji", "input": input, "cps": cps, "emoji": emoji })
            }
            Token::Nfc {
                input,
                tokens0,
                cps,
                tokens,
            } => json!({
                "type": "nfc",
                "input": input,
                "tokens0": tokens0.iter().map(token_to_value).collect::<Vec<_>>(),
                "cps": cps,
                "tokens": tokens.iter().map(token_to_value).collect::<Vec<_>>()
            }),
        }
    }

    #[wasm_bindgen(js_name = ens_normalize)]
    pub fn ens_normalize_js(name: &str) -> std::result::Result<String, JsValue> {
        crate::ens_normalize(name).map_err(js_error)
    }

    #[wasm_bindgen(js_name = ens_beautify)]
    pub fn ens_beautify_js(name: &str) -> std::result::Result<String, JsValue> {
        crate::ens_beautify(name).map_err(js_error)
    }

    #[wasm_bindgen(js_name = ens_normalize_fragment)]
    pub fn ens_normalize_fragment_js(
        fragment: &str,
        decompose: bool,
    ) -> std::result::Result<String, JsValue> {
        crate::ens_normalize_fragment(fragment, decompose).map_err(js_error)
    }

    #[wasm_bindgen(js_name = ens_tokenize)]
    pub fn ens_tokenize_js(name: &str) -> std::result::Result<JsValue, JsValue> {
        let tokens = crate::ens_tokenize(name)
            .iter()
            .map(token_to_value)
            .collect::<Vec<_>>();
        to_js(&tokens)
    }

    #[wasm_bindgen(js_name = ens_emoji)]
    pub fn ens_emoji_js() -> std::result::Result<JsValue, JsValue> {
        to_js(&crate::ens_emoji())
    }

    #[wasm_bindgen(js_name = nfc)]
    pub fn nfc_js(cps: JsValue) -> std::result::Result<JsValue, JsValue> {
        let cps: Vec<u32> = serde_wasm_bindgen::from_value(cps)
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
        to_js(&crate::nfc(&cps))
    }

    #[wasm_bindgen(js_name = nfd)]
    pub fn nfd_js(cps: JsValue) -> std::result::Result<JsValue, JsValue> {
        let cps: Vec<u32> = serde_wasm_bindgen::from_value(cps)
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
        to_js(&crate::nfd(&cps))
    }
}
