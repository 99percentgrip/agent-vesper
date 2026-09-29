use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use serde_json::json;
use vesper_agent::executor::uncancellable_context;
use vesper_agent::{
    ToolContext, ToolError, ToolFuture, ToolRegistry, ToolResult, ToolService, schema_definition,
};
use vesper_domain::{
    HarnessToolName, ProviderId, SessionOperatingMode, SessionPermissionMode, ToolCall, ToolCallId,
    ToolDefinition, ToolExecutionClass, ToolId, ToolProviderScope,
};

struct ScopedService(Arc<AtomicUsize>);

impl ToolService for ScopedService {
    fn definitions(&self) -> Vec<ToolDefinition> {
        let mut definition = schema_definition(
            "provider_web",
            "Provider-backed web capability.",
            ToolExecutionClass::ReadOnly,
            &[],
        );
        definition.provider_scope =
            ToolProviderScope::Provider(ProviderId::new("owner").expect("fixture provider"));
        vec![definition]
    }

    fn execute<'a>(
        &'a self,
        _call: &'a ToolCall,
        _context: &'a ToolContext,
    ) -> ToolFuture<'a, Result<ToolResult, ToolError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { ToolResult::new("executed") })
    }
}

fn call() -> ToolCall {
    ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        tool_id: ToolId::new("provider_web").unwrap(),
        arguments: json!({}),
        extensions: Default::default(),
    }
}

#[tokio::test]
async fn provider_scope_filters_advertisement_and_blocks_forged_execution() {
    let executions = Arc::new(AtomicUsize::new(0));
    let registry = ToolRegistry::empty().with_service(Arc::new(ScopedService(executions.clone())));
    let owner = ProviderId::new("owner").unwrap();
    let other = ProviderId::new("other").unwrap();

    assert_eq!(
        registry
            .definitions_for_provider(SessionOperatingMode::Code, &owner)
            .iter()
            .map(|definition| definition.harness_name.clone())
            .collect::<Vec<HarnessToolName>>(),
        vec![HarnessToolName::new("provider_web").unwrap()]
    );
    assert!(
        registry
            .definitions_for_provider(SessionOperatingMode::Code, &other)
            .is_empty()
    );

    let mut context = uncancellable_context(
        Vec::new(),
        SessionOperatingMode::Code,
        SessionPermissionMode::Bypass,
    );
    context.provider_id = other;
    let error = registry.execute(&call(), &context).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unavailable for active provider `other`")
    );
    assert_eq!(executions.load(Ordering::SeqCst), 0);

    context.provider_id = owner;
    assert_eq!(
        registry
            .execute(&call(), &context)
            .await
            .unwrap()
            .text
            .as_str(),
        "executed"
    );
    assert_eq!(executions.load(Ordering::SeqCst), 1);

    context.provider_id = ProviderId::new("other").unwrap();
    assert!(
        registry
            .definitions_for_provider(SessionOperatingMode::Code, &context.provider_id)
            .is_empty()
    );
    context.provider_id = ProviderId::new("owner").unwrap();
    assert_eq!(
        registry
            .definitions_for_provider(SessionOperatingMode::Code, &context.provider_id)
            .len(),
        1
    );
}
