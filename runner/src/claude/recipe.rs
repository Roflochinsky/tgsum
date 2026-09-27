use super::{ClaudeRequest, DecodeError, Decoded};
use crate::{Cancellation, PreparedContext, RunOutput, RunnerError};
use tgsum_core::recipe::{Recipe, RecipeEvidence, RecipeOutput};

/// The compiled task, schema and local validator remain one operation.
pub struct RecipeRequest<'a> {
    request: ClaudeRequest<'a>,
    recipe: Recipe,
    evidence: RecipeEvidence,
}
impl<'a> RecipeRequest<'a> {
    pub fn prepare(
        context: &'a PreparedContext,
        model: &str,
        recipe: Recipe,
        cancel: &Cancellation,
    ) -> Result<Self, RunnerError> {
        let documents = context.documents(super::MAX_INPUT_BYTES, cancel)?;
        let evidence = RecipeEvidence::from_markdown(
            documents
                .iter()
                .filter(|d| d.name.ends_with(".md"))
                .map(|d| d.text.as_str()),
        )?;
        let request =
            ClaudeRequest::prepare(context, model, &recipe.task(), &recipe.schema(), cancel)?;
        Ok(Self {
            request,
            recipe,
            evidence,
        })
    }
    pub fn request(&self) -> &ClaudeRequest<'a> {
        &self.request
    }
    pub fn recipe(&self) -> Recipe {
        self.recipe
    }
    pub fn validate(
        &self,
        value: &RecipeOutput,
    ) -> std::io::Result<Vec<tgsum_core::bundle::EvidenceRef>> {
        self.recipe.validate(value, &self.evidence)
    }
    pub fn decode(&self, output: &RunOutput) -> Result<Decoded<RecipeOutput>, DecodeError> {
        super::decode(output, self.request.model(), |value| {
            self.validate(value).is_ok()
        })
    }
}
