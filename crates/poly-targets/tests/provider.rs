//! T-M8-05：统一 provider 注册表契约（注册顺序、去重、重复拒绝、声明校验）。

use poly_targets::provider::{
    PackageArtifact, PackageInput, ProviderError, ProviderRegistry, TargetProvider,
};
use poly_targets::{Host, TargetSpec};

struct Fixed(TargetSpec, &'static [u8]);

impl TargetProvider for Fixed {
    fn describe(&self) -> Result<TargetSpec, ProviderError> {
        Ok(self.0.clone())
    }

    fn package(&self, input: &PackageInput<'_>) -> Result<PackageArtifact, ProviderError> {
        Ok(PackageArtifact::inline(
            input.target.to_string(),
            self.0.package.format.clone(),
            self.1.to_vec(),
        ))
    }
}

fn spec(id: &str, format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub(id, 3, &[Host::Linux]);
    spec.package.format = format.to_string();
    spec
}

fn provider(id: &str, format: &str, bytes: &'static [u8]) -> Box<dyn TargetProvider> {
    Box::new(Fixed(spec(id, format), bytes))
}

#[test]
fn registers_by_target_and_format_in_order() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(provider("demo", "bin", b"a"))
        .expect("首个格式");
    registry
        .register(provider("demo", "zip", b"b"))
        .expect("同目标另一格式");

    assert_eq!(
        registry.keys(),
        [
            ("demo".to_string(), "bin".to_string()),
            ("demo".to_string(), "zip".to_string())
        ]
    );
    // `specs()` 按 id 去重，保留先注册的默认格式
    let specs = registry.specs();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].package.format, "bin");

    let input = PackageInput {
        name: "demo",
        version: "1.0.0",
        target: "demo",
        entry: "index.html",
        files: &[],
        triple: "native",
        lib: &[],
        host: None,
    };
    let artifact = registry
        .get("demo", "zip")
        .expect("按格式命中")
        .package(&input)
        .expect("打包");
    assert_eq!(artifact.format, "zip");
    assert_eq!(artifact.content, b"b");
    assert!(registry.get("demo", "msi").is_none());
}

#[test]
fn duplicate_key_is_rejected() {
    let mut registry = ProviderRegistry::new();
    registry
        .register(provider("demo", "bin", b"a"))
        .expect("首次注册");
    let err = registry
        .register(provider("demo", "bin", b"b"))
        .expect_err("重复（id，格式）必须拒绝");
    assert!(matches!(err, ProviderError::Duplicate { .. }));
    assert_eq!(registry.len(), 1);
}

#[test]
fn invalid_declaration_is_rejected_at_registration() {
    let mut registry = ProviderRegistry::new();
    let mut bad = spec("Demo Target", "bin");
    bad.id = "Demo Target".to_string();
    let err = registry
        .register(Box::new(Fixed(bad, b"x")))
        .expect_err("非法 id 必须拒绝");
    assert!(matches!(err, ProviderError::Spec(_)));
    assert!(registry.is_empty());
}
