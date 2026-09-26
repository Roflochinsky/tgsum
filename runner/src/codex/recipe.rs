//! Bind the compiled recipe's instructions, schema and validator to one Review.
use super::CodexRequest;
use crate::{Cancellation, PreparedContext, RunnerError};
use tgsum_core::recipe::{Recipe, RecipeEvidence};

pub struct RecipeRequest<'a> {
    request: CodexRequest<'a>,
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
            CodexRequest::prepare(context, model, &recipe.task(), &recipe.schema(), cancel)?;
        Ok(Self {
            request,
            recipe,
            evidence,
        })
    }

    pub fn request(&self) -> &CodexRequest<'a> {
        &self.request
    }
    pub fn recipe(&self) -> Recipe {
        self.recipe
    }

    pub fn validate(
        &self,
        output: &tgsum_core::recipe::RecipeOutput,
    ) -> std::io::Result<Vec<tgsum_core::bundle::EvidenceRef>> {
        self.recipe.validate(output, &self.evidence)
    }

    #[cfg(target_os = "linux")]
    pub fn job(
        &'a self,
        ticket: &'a tgsum_core::analysis::RunTicket,
    ) -> Result<super::AnalysisJob<'a>, super::AnalysisError> {
        let spec = &ticket.request().spec;
        if spec.recipe != self.recipe.id()
            || spec.recipe_version != tgsum_core::recipe::RECIPE_VERSION
        {
            return Err(super::AnalysisError::Binding);
        }
        super::AnalysisJob::new(&self.request, ticket)
    }
}

#[cfg(target_os = "linux")]
impl super::CodexNetworkRunner {
    pub fn run_recipe(
        &self,
        request: &RecipeRequest<'_>,
        ticket: &tgsum_core::analysis::RunTicket,
        auth: &super::SelectedAuthFile,
        cancel: &Cancellation,
    ) -> Result<super::CompletedAnalysis<tgsum_core::recipe::RecipeOutput>, super::AnalysisError>
    {
        self.run_analysis(
            request.job(ticket)?,
            auth,
            super::run_limits(),
            cancel,
            |value| request.validate(value),
        )
    }
}
