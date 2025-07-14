use crate::mutation;
use derive_more::Display;
use derive_more::Error;
use hora::core::ann_index::ANNIndex;
use hora::index::hnsw_idx::HNSWIndex;
use std::collections::HashMap;
use tree_sitter::Parser;

#[derive(Debug, Display, Error)]
pub enum Error {
    #[display("failed to embed prompt")]
    EmbedFailed,
    #[display("snippets were requested for an unknown language")]
    UnknownLang,
    #[display("failed to parse corpus of code to apply mutation on")]
    SnippetParsing,
}

pub struct Refactor {
    pub dict: HashMap<String, HNSWIndex<f32, usize>>,
    pub mutations_collection: Vec<mutation::MutationCollection>,
}

impl Refactor {
    fn get_lang(s: &str) -> Result<tree_sitter::Language, Error> {
        Ok(match s {
            "go" => tree_sitter_go::LANGUAGE,
            "c" => tree_sitter_c::LANGUAGE,
            "rs" => tree_sitter_rust::LANGUAGE,
            _ => return Err(Error::UnknownLang),
        }
        .into())
    }

    pub fn search(
        &self,
        lang: &str,
        target: &[f32],
        body: &str,
        top_k: usize,
    ) -> Result<Vec<String>, Error> {
        let langfn = Self::get_lang(lang)?;
        let mut parser = Parser::new();
        parser
            .set_language(&langfn)
            .map_err(|_| Error::UnknownLang)?;

        let source_code = body;
        let source_bytes = source_code.as_bytes();
        let tree = parser
            .parse(source_code, None)
            .ok_or(Error::SnippetParsing)?;
        let root_node = tree.root_node();

        // search for k nearest neighbors
        let collected = self.dict[lang]
            .search(target, top_k)
            .iter()
            .filter_map(|&index| {
                let applied = mutation::apply(
                    langfn.clone(),
                    source_bytes,
                    root_node,
                    &self.mutations_collection[index],
                );
                match applied {
                    Ok(v) => Some(v),
                    Err(e) => {
                        tracing::error!(
                            collection_index = index,
                            "failed to apply mutations from collection {}",
                            e
                        );
                        None
                    }
                }
            })
            .collect();
        Ok(collected)
    }
}
pub struct Generate {
    pub dict: HashMap<String, HNSWIndex<f32, String>>,
}

impl Generate {
    fn search(&self, lang: &str, target: &[f32], top_k: usize) -> Result<Vec<String>, Error> {
        let Some(snippets_for_lang) = self.dict.get(lang) else {
            return Err(Error::UnknownLang);
        };
        Ok(snippets_for_lang.search(target, top_k))
    }
}

pub struct State {
    // TODO: create new constructor and private these fields
    pub embed: crate::embed::Embed,
    pub generate: Generate,
    pub refactor: Refactor,
}

impl State {
    pub fn generate(&self, lang: &str, prompt: &str, top_k: usize) -> Result<Vec<String>, Error> {
        let Ok(target) = self.embed.embed(prompt) else {
            return Err(Error::EmbedFailed);
        };

        self.generate.search(lang, &target, top_k)
    }

    pub fn refactor(
        &self,
        lang: &str,
        prompt: &str,
        body: &str,
        top_k: usize,
    ) -> Result<Vec<String>, Error> {
        let Ok(target) = self.embed.embed(prompt) else {
            return Err(Error::EmbedFailed);
        };

        self.refactor.search(lang, &target, body, top_k)
    }
}
