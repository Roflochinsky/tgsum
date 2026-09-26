//! Inspect the compiled recipe catalog without a Project, account or network.
use serde_json::json;
use tgsum_core::recipe::{Recipe, RECIPE_VERSION};

fn main() {
    let catalog: Vec<_> = Recipe::ALL.into_iter().map(|r| json!({
        "id":r.id(), "title":r.title(), "version":RECIPE_VERSION,
        "task":r.task(), "schema":r.schema(),
        "empty_result":{"recipe":r.id(),"version":RECIPE_VERSION,
            "sections":r.sections().iter().map(|id|json!({"id":id,"claims":[]})).collect::<Vec<_>>(),
            "actions":[]}
    })).collect();
    println!("{}", serde_json::to_string_pretty(&catalog).unwrap());
}
