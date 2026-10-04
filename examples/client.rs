use temporal::prelude::*;
struct Greeting;
impl WorkflowDefinition for Greeting {
    type Input = String;
    type Output = String;
    fn name(&self) -> &str {
        "greeting"
    }
}
impl HasWorkflowDefinition for Greeting {
    type Run = Self;
}
struct Ready;
impl SignalDefinition for Ready {
    type Workflow = Greeting;
    type Input = ();
    fn name(&self) -> &str {
        "ready"
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client =
        temporal::client::connect("http://localhost:7233".parse()?, "default", "example", "1")
            .await?;
    let handle = client
        .signal_with_start_workflow(
            Greeting,
            "Rust".into(),
            Ready,
            (),
            WorkflowStartOptions::new("example", "greeting-example").build(),
        )
        .await?;
    println!(
        "{}",
        handle
            .get_result(WorkflowGetResultOptions::default())
            .await?
    );
    Ok(())
}
