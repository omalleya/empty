//! LLM layer: turn free-text shopping-list lines into concrete Kroger UPCs.
//!
//! Two jobs, both handled by Claude:
//!
//!   1. Phrase a good *search term* for the Products API
//!      ("a couple gallons of 2% milk"  ->  "2% milk gallon")
//!
//!   2. *Select* the best matching product from the search results
//!      (dozens of milk UPCs -> the one the user meant)
//!
//! A JSON cache (`upc_cache.json`) remembers raw_text -> UPC so each line is only
//! disambiguated once. First run you confirm; every run after is instant + free.
//!
//! Calls the Messages API (`POST /v1/messages`) over raw HTTP — there is no
//! official Rust SDK. Structured outputs via `output_config.format` with a
//! JSON schema; the first text block is then guaranteed-valid JSON.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{anyhow, Context};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::kroger::{KrogerClient, Product};

const MODEL: &str = "claude-opus-4-8";
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

// Leading quantity like "2 milk", "2x milk", "milk x2"
static QTY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*(\d+)\s*x?\s+|\s+x\s*(\d+)\s*$").unwrap());

/// Result of turning one free-text list line into a concrete product.
pub struct ResolvedItem {
    pub raw_text: String,
    pub quantity: u32,
    #[allow(dead_code)] // kept for parity with the Python model; handy in debugging
    pub search_term: String,
    pub chosen: Option<Product>,
    pub candidates: Vec<Product>,
    pub from_cache: bool,
    pub note: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    upc: String,
    #[serde(default)]
    label: String,
}

// ----------------------- structured-output schemas ----------------------- //
#[derive(serde::Deserialize)]
struct SearchTerm {
    raw: String,
    term: String,
}

#[derive(serde::Deserialize)]
struct SearchTerms {
    items: Vec<SearchTerm>,
}

fn search_terms_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "raw": {"type": "string", "description": "The original list line, verbatim."},
                        "term": {"type": "string", "description": "A concise grocery search term for that line."}
                    },
                    "required": ["raw", "term"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["items"],
        "additionalProperties": false
    })
}

#[derive(serde::Deserialize)]
struct Selection {
    chosen_index: i64,
    confidence: f64,
    reason: String,
}

fn selection_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "chosen_index": {
                "type": "integer",
                "description": "0-based index of the best matching product, or -1 if none fit."
            },
            "confidence": {"type": "number", "description": "0.0-1.0 confidence in the choice."},
            "reason": {"type": "string", "description": "One short sentence explaining the pick."}
        },
        "required": ["chosen_index", "confidence", "reason"],
        "additionalProperties": false
    })
}

pub struct Resolver {
    api_key: String,
    http: reqwest::Client,
    cache_path: PathBuf,
    cache: BTreeMap<String, CacheEntry>,
    auto_accept_confidence: f64,
}

impl Resolver {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::Config("ANTHROPIC_API_KEY is not set".into()))?;
        let cache_path = PathBuf::from(
            std::env::var("KROGER_UPC_CACHE").unwrap_or_else(|_| "upc_cache.json".into()),
        );
        let cache = std::fs::read_to_string(&cache_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Ok(Self {
            api_key,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .map_err(Error::Http)?,
            cache_path,
            cache,
            auto_accept_confidence: 0.75,
        })
    }

    // ------------------------------------------------------------------ //
    // public entry point
    // ------------------------------------------------------------------ //
    pub async fn resolve(
        &mut self,
        lines: &[String],
        client: &KrogerClient,
    ) -> Result<Vec<ResolvedItem>> {
        let parsed: Vec<(String, u32)> = lines.iter().map(|l| split_quantity(l)).collect();

        // Batch the search-term phrasing for all not-yet-cached lines in one call.
        let misses: Vec<String> = parsed
            .iter()
            .filter(|(text, _)| !self.cache.contains_key(&cache_key(text)))
            .map(|(text, _)| text.clone())
            .collect();
        let terms = if misses.is_empty() {
            HashMap::new()
        } else {
            self.search_terms(&misses).await?
        };

        let mut results = Vec::with_capacity(parsed.len());
        for (text, qty) in parsed {
            let key = cache_key(&text);
            if let Some(entry) = self.cache.get(&key) {
                results.push(ResolvedItem {
                    raw_text: text.clone(),
                    quantity: qty,
                    search_term: String::new(),
                    chosen: Some(Product {
                        upc: entry.upc.clone(),
                        description: if entry.label.is_empty() {
                            text
                        } else {
                            entry.label.clone()
                        },
                        brand: String::new(),
                        size: String::new(),
                        price: None,
                    }),
                    candidates: vec![],
                    from_cache: true,
                    note: String::new(),
                });
                continue;
            }

            let term = terms.get(&text).cloned().unwrap_or_else(|| text.clone());
            let candidates = client.search_products(&term, 10).await?;
            let mut item = ResolvedItem {
                raw_text: text.clone(),
                quantity: qty,
                search_term: term,
                chosen: None,
                candidates,
                from_cache: false,
                note: String::new(),
            };
            if item.candidates.is_empty() {
                item.note = "no search results".into();
                results.push(item);
                continue;
            }

            let sel = self.select(&text, &item.candidates).await?;
            let idx = usize::try_from(sel.chosen_index).ok();
            match idx.filter(|i| *i < item.candidates.len()) {
                Some(i) if sel.confidence >= self.auto_accept_confidence => {
                    let chosen = item.candidates[i].clone();
                    self.cache.insert(
                        key,
                        CacheEntry {
                            upc: chosen.upc.clone(),
                            label: chosen.label(),
                        },
                    );
                    item.chosen = Some(chosen);
                    item.note = sel.reason;
                }
                _ => {
                    // Low confidence or no fit — leave unchosen for the caller to confirm.
                    item.note = format!("needs confirmation ({})", sel.reason);
                }
            }
            results.push(item);
        }

        self.save_cache();
        Ok(results)
    }

    // ------------------------------------------------------------------ //
    // LLM calls
    // ------------------------------------------------------------------ //
    async fn search_terms(&self, lines: &[String]) -> Result<HashMap<String, String>> {
        let listing = lines
            .iter()
            .map(|l| format!("- {l}"))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "Convert each grocery shopping-list line into a short search term suited \
             to a grocery store product search. Drop quantities and filler words; keep \
             brand, variety, and size cues.\n\n{listing}"
        );
        let out: SearchTerms = self.parse_llm(&prompt, 1024, search_terms_schema()).await?;
        let mapping: HashMap<String, String> =
            out.items.into_iter().map(|st| (st.raw, st.term)).collect();
        // Fall back to the raw line for anything the model didn't echo back.
        Ok(lines
            .iter()
            .map(|l| (l.clone(), mapping.get(l).cloned().unwrap_or_else(|| l.clone())))
            .collect())
    }

    async fn select(&self, raw_text: &str, candidates: &[Product]) -> Result<Selection> {
        let listing = candidates
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{i}: {}", p.label()))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "Shopping-list item: \"{raw_text}\"\n\n\
             Candidate products:\n{listing}\n\n\
             Pick the single best match for what the shopper most likely wants. \
             Prefer the common/default size and variety unless the item specifies \
             otherwise. If nothing is a reasonable match, return chosen_index -1."
        );
        self.parse_llm(&prompt, 512, selection_schema()).await
    }

    /// One structured-output Messages API call: the schema constrains the
    /// response, so the first text block parses directly into T.
    async fn parse_llm<T: DeserializeOwned>(
        &self,
        prompt: &str,
        max_tokens: u32,
        schema: Value,
    ) -> Result<T> {
        let body = json!({
            "model": MODEL,
            "max_tokens": max_tokens,
            "output_config": {
                "effort": "low",
                "format": {"type": "json_schema", "schema": schema},
            },
            "messages": [{"role": "user", "content": prompt}],
        });
        let resp = self
            .http
            .post(ANTHROPIC_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let detail = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Anthropic API error ({status}): {detail}").into());
        }
        let message: Value = resp.json().await?;
        let text = message["content"]
            .as_array()
            .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
            .and_then(|b| b["text"].as_str())
            .ok_or_else(|| {
                anyhow!(
                    "no text block in Anthropic response (stop_reason: {})",
                    message["stop_reason"]
                )
            })?;
        Ok(serde_json::from_str(text).context("parsing structured output")?)
    }

    // ------------------------------------------------------------------ //
    // cache
    // ------------------------------------------------------------------ //
    fn save_cache(&self) {
        if let Ok(serialized) = serde_json::to_string_pretty(&self.cache) {
            if let Err(err) = std::fs::write(&self.cache_path, serialized) {
                eprintln!(
                    "warning: failed to write UPC cache {}: {err}",
                    self.cache_path.display()
                );
            }
        }
    }
}

fn split_quantity(line: &str) -> (String, u32) {
    if let Some(caps) = QTY_RE.captures(line) {
        let qty = caps
            .get(1)
            .or_else(|| caps.get(2))
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let text = QTY_RE.replace_all(line, " ").trim().to_string();
        (text, qty)
    } else {
        (line.trim().to_string(), 1)
    }
}

fn cache_key(text: &str) -> String {
    text.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}
