use crate::mutation;
use derive_more::Display;
use derive_more::Error;
use hora::core::ann_index::ANNIndex;
use hora::index::hnsw_idx::HNSWIndex;
use std::collections::HashMap;
use std::path::Path;
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
    pub fn search(
        &self,
        lang: &str,
        target: &[f32],
        body: &str,
        top_k: usize,
    ) -> Result<Vec<String>, Error> {
        let langfn = lang_from_name(lang)?;
        let source_bytes = body.as_bytes();
        let tree = parse_into_tree(source_bytes, &langfn)?;
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

pub fn lang_from_name(s: &str) -> Result<tree_sitter::Language, Error> {
    Ok(match s {
        "go" => tree_sitter_go::LANGUAGE,
        "c" | "h" => tree_sitter_c::LANGUAGE,
        "cpp" | "hpp" => tree_sitter_cpp::LANGUAGE,
        "js" | "ts" => tree_sitter_javascript::LANGUAGE,
        "rs" => tree_sitter_rust::LANGUAGE,
        _ => return Err(Error::UnknownLang),
    }
    .into())
}

pub fn lang_from_file_extension(path: &Path) -> Result<tree_sitter::Language, Error> {
    let Some(lang) = path.extension() else {
        return Err(Error::UnknownLang);
    };
    let lang = lang.to_str().ok_or(Error::UnknownLang)?;
    lang_from_name(lang)
}

// parses `body` written in the language `langfn` into tree sitter AST
pub fn parse_into_tree(
    body: &[u8],
    langfn: &tree_sitter::Language,
) -> Result<tree_sitter::Tree, Error> {
    let mut parser = Parser::new();
    parser
        .set_language(langfn)
        .map_err(|_| Error::UnknownLang)?;
    let tree = parser.parse(body, None).ok_or(Error::SnippetParsing)?;
    Ok(tree)
}

pub fn dump_expression(path: &Path) -> Result<String, Error> {
    let source_bytes = std::fs::read(path).map_err(|_| Error::SnippetParsing)?;

    let tree = parse_into_tree(&source_bytes, &lang_from_file_extension(path)?)?;

    Ok(tree.root_node().to_sexp().to_string())
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
    pub embeddings: crate::embed::Embed,
    pub generate: Generate,
    pub refactor: Refactor,
}

impl State {
    pub fn embed(&self, prompt: &str) -> Result<Vec<f32>, Error> {
        self.embeddings
            .embed(prompt)
            .map_err(|_| Error::EmbedFailed)
    }
    pub fn generate(&self, lang: &str, prompt: &str, top_k: usize) -> Result<Vec<String>, Error> {
        self.generate.search(lang, &self.embed(prompt)?, top_k)
    }

    pub fn refactor(
        &self,
        lang: &str,
        prompt: &str,
        body: &str,
        top_k: usize,
    ) -> Result<Vec<String>, Error> {
        self.refactor
            .search(lang, &self.embed(prompt)?, body, top_k)
    }
}
