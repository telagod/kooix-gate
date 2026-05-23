use anyhow::{Context, bail};
use gate_providers::types::{ChatMessage, ChatRequest, Role};
use gate_providers::{CustomHttpProvider, Provider};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const REGISTRY_SCHEMA_VERSION: u8 = 1;
const PACKAGE_SCHEMA_VERSION: u8 = 1;
const DEFAULT_OFFICIAL_REGISTRY_ROOT: &str = "examples/manifest-registry";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginFixture {
    version: u8,
    base_url: String,
    model: String,
    manifest: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_sample: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_sse: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_chunks: Option<Vec<gate_providers::types::ChatStreamChunk>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestRegistry {
    schema_version: u8,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    entries: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryEntry {
    id: String,
    name: String,
    version: String,
    author: String,
    source: RegistrySource,
    manifest_path: String,
    sha256: String,
    compatibility: RegistryCompatibility,
    signature: RegistrySignature,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RegistrySource {
    Official,
    Community,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistryCompatibility {
    min_gate_version: String,
    #[serde(default)]
    max_gate_version: Option<String>,
    manifest_schema: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistrySignature {
    kind: SignatureKind,
    #[serde(default)]
    value: Option<String>,
    /// 0.4.53: pubkey fingerprint / cert identity for verify
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_id: Option<String>,
    /// 0.4.53: ed25519 / rsa-pss-sha256 / ecdsa-p256-sha256
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SignatureKind {
    Unsigned,
    Cosign,
    Minisign,
    SigstoreBundle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestPackage {
    schema_version: u8,
    registry: RegistryEntry,
    manifest: serde_json::Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fixtures: Vec<PluginFixture>,
    readme: String,
    security: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageManifest {
    schema_version: u8,
    id: String,
    name: String,
    version: String,
    author: String,
    source: RegistrySource,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    description: Option<String>,
    compatibility: RegistryCompatibility,
    signature: RegistrySignature,
    contents: PackageContents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageContents {
    manifest: String,
    readme: String,
    security: String,
    fixtures: String,
}

#[derive(Debug, Clone, Serialize)]
struct PackageLintReport {
    id: String,
    version: String,
    manifest: String,
    readme: String,
    security: String,
    fixtures_dir: String,
    fixtures: Vec<String>,
    sha256: String,
    verified: bool,
}

#[derive(Debug, Clone)]
pub struct PackageLintInput {
    pub root: String,
    pub verify: bool,
    pub json: bool,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct RegistryPackageInput {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub source: String,
    pub manifest_path: String,
    pub output: String,
    pub readme_path: Option<String>,
    pub security_path: Option<String>,
    pub fixture_paths: Vec<String>,
    pub min_gate_version: String,
    pub max_gate_version: Option<String>,
    pub signature_kind: String,
    pub signature_value: Option<String>,
    pub homepage: Option<String>,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct RegistryExportInput {
    pub registry_root: String,
    pub output: Option<String>,
    pub include_private: bool,
}

#[derive(Debug, Clone)]
pub struct RegistryImportInput {
    pub package_path: String,
    pub registry_root: String,
    pub private_namespace: String,
    pub verify: bool,
    pub allow_unsigned: bool,
    pub base_url: String,
}

pub fn schema() -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&gate_providers::plugin_manifest_schema_json())?
    );
    Ok(())
}

pub fn lint(path: Option<String>, base_url: String) -> anyhow::Result<()> {
    let input = read_manifest(path)?;
    let value: serde_json::Value =
        serde_json::from_str(&input).context("plugin manifest must be valid JSON")?;
    gate_providers::validate_plugin_manifest(value, &base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("plugin manifest ok");
    Ok(())
}

pub fn replay(
    manifest_path: Option<String>,
    sse_path: String,
    base_url: String,
    model: String,
) -> anyhow::Result<()> {
    let input = read_manifest(manifest_path)?;
    let value: serde_json::Value =
        serde_json::from_str(&input).context("plugin manifest must be valid JSON")?;
    let raw_sse = fs::read(&sse_path).with_context(|| format!("read {sse_path}"))?;
    let chunks = gate_providers::replay_plugin_sse(value, &base_url, raw_sse, &model)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("{}", serde_json::to_string_pretty(&chunks)?);
    Ok(())
}

pub fn registry_list(root: Option<String>, json: bool) -> anyhow::Result<()> {
    let root = root.unwrap_or_else(|| DEFAULT_OFFICIAL_REGISTRY_ROOT.to_string());
    let registry = load_registry_from_root(&root)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&registry)?);
        return Ok(());
    }
    println!("{} — {} manifest(s)", registry.name, registry.entries.len());
    for entry in &registry.entries {
        println!(
            "- {}@{} [{}] by {} ({})",
            entry.id,
            entry.version,
            source_str(&entry.source),
            entry.author,
            entry.compatibility.min_gate_version
        );
    }
    Ok(())
}

pub fn package_lint(input: PackageLintInput) -> anyhow::Result<()> {
    let report = lint_package_dir(&input)?;
    if input.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!(
            "plugin package ok: {}@{} ({} fixture(s), sha256 {}, verified={})",
            report.id,
            report.version,
            report.fixtures.len(),
            report.sha256,
            report.verified
        );
    }
    Ok(())
}

pub fn registry_package(input: RegistryPackageInput) -> anyhow::Result<()> {
    let manifest_input = fs::read_to_string(&input.manifest_path)
        .with_context(|| format!("read {}", input.manifest_path))?;
    let manifest = parse_manifest(manifest_input)?;
    gate_providers::validate_plugin_manifest(manifest.clone(), &input.base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    validate_registry_id(&input.id)?;
    validate_version(&input.version, "version")?;
    validate_version(&input.min_gate_version, "min_gate_version")?;
    if let Some(max) = input.max_gate_version.as_deref() {
        validate_version(max, "max_gate_version")?;
    }
    let source = parse_source(&input.source)?;
    let signature = RegistrySignature {
        kind: parse_signature_kind(&input.signature_kind)?,
        value: input.signature_value.filter(|v| !v.trim().is_empty()),
        key_id: None,
        alg: None,
    };
    if signature.kind != SignatureKind::Unsigned && signature.value.is_none() {
        bail!("signature value is required for {:?}", signature.kind);
    }
    let readme = input
        .readme_path
        .as_deref()
        .map(|p| fs::read_to_string(p).with_context(|| format!("read {p}")))
        .transpose()?
        .unwrap_or_else(|| default_package_readme(&input.name, &input.id));
    let security = input
        .security_path
        .as_deref()
        .map(|p| fs::read_to_string(p).with_context(|| format!("read {p}")))
        .transpose()?
        .unwrap_or_else(default_package_security);
    let mut fixtures = Vec::with_capacity(input.fixture_paths.len());
    for path in &input.fixture_paths {
        let fixture_input = fs::read_to_string(path).with_context(|| format!("read {path}"))?;
        let fixture: PluginFixture = serde_json::from_str(&fixture_input)
            .with_context(|| format!("{path} must be a plugin fixture JSON"))?;
        validate_fixture(&fixture)?;
        fixtures.push(fixture);
    }
    let manifest_sha = manifest_digest(&manifest)?;
    let package = ManifestPackage {
        schema_version: PACKAGE_SCHEMA_VERSION,
        registry: RegistryEntry {
            id: input.id,
            name: input.name,
            version: input.version,
            author: input.author,
            source,
            manifest_path: "manifest.json".to_string(),
            sha256: manifest_sha,
            compatibility: RegistryCompatibility {
                min_gate_version: input.min_gate_version,
                max_gate_version: input.max_gate_version,
                manifest_schema: 1,
            },
            signature,
            homepage: input.homepage,
            tags: input.tags,
            description: input.description,
        },
        manifest,
        fixtures,
        readme,
        security,
    };
    write_output(Some(input.output), &serde_json::to_string_pretty(&package)?)?;
    Ok(())
}

fn lint_package_dir(input: &PackageLintInput) -> anyhow::Result<PackageLintReport> {
    let root = Path::new(&input.root);
    if !root.is_dir() {
        bail!("package root must be a directory: {}", root.display());
    }

    let package_path = root.join("package.json");
    let package_value = read_json_file(&package_path.to_string_lossy())?;
    let package: PackageManifest = serde_json::from_value(package_value).with_context(|| {
        format!(
            "{} must be a manifest package spec JSON",
            package_path.display()
        )
    })?;
    if package.schema_version != PACKAGE_SCHEMA_VERSION {
        bail!(
            "unsupported package schema_version: {}",
            package.schema_version
        );
    }
    validate_registry_id(&package.id)?;
    validate_version(&package.version, "version")?;
    validate_compatibility(&package.compatibility, &package.id)?;
    validate_signature(&package.signature, &package.id, true)?;

    let manifest_rel = validate_named_relative_path(&package.contents.manifest, "manifest.json")?;
    let readme_rel = validate_named_relative_path(&package.contents.readme, "README.md")?;
    let security_rel = validate_named_relative_path(&package.contents.security, "security.md")?;
    let fixtures_rel = validate_relative_dir_path(&package.contents.fixtures, "fixtures")?;

    let manifest_path = root.join(manifest_rel);
    let readme_path = root.join(readme_rel);
    let security_path = root.join(security_rel);
    let fixtures_dir = root.join(fixtures_rel);

    let manifest = read_json_file(&manifest_path.to_string_lossy())?;
    gate_providers::validate_plugin_manifest(manifest.clone(), &input.base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let sha256 = manifest_digest(&manifest)?;

    ensure_markdown_contains(&readme_path, &package.id, "README.md")?;
    ensure_markdown_contains(&security_path, "Security", "security.md")?;
    ensure_markdown_contains(&security_path, "secret", "security.md")?;

    if !fixtures_dir.is_dir() {
        bail!(
            "fixtures path must be a directory: {}",
            fixtures_dir.display()
        );
    }
    let fixtures = load_package_fixtures(&fixtures_dir)?;
    if fixtures.is_empty() {
        bail!("package fixtures/ must contain at least one *.fixture.json file");
    }
    for fixture in &fixtures {
        validate_fixture(&fixture.fixture)?;
        if fixture.fixture.manifest != manifest {
            bail!(
                "fixture {} manifest must match package manifest.json",
                fixture.path.display()
            );
        }
        if input.verify {
            verify_fixture_replay(&fixture.fixture)
                .with_context(|| format!("verify {}", fixture.path.display()))?;
        }
    }

    Ok(PackageLintReport {
        id: package.id,
        version: package.version,
        manifest: path_for_registry(&manifest_path, &input.root),
        readme: path_for_registry(&readme_path, &input.root),
        security: path_for_registry(&security_path, &input.root),
        fixtures_dir: path_for_registry(&fixtures_dir, &input.root),
        fixtures: fixtures
            .into_iter()
            .map(|fixture| path_for_registry(&fixture.path, &input.root))
            .collect(),
        sha256,
        verified: input.verify,
    })
}

struct LoadedPackageFixture {
    path: PathBuf,
    fixture: PluginFixture,
}

fn load_package_fixtures(fixtures_dir: &Path) -> anyhow::Result<Vec<LoadedPackageFixture>> {
    let mut paths = Vec::new();
    for entry in
        fs::read_dir(fixtures_dir).with_context(|| format!("read {}", fixtures_dir.display()))?
    {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".fixture.json"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let input =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            let fixture: PluginFixture = serde_json::from_str(&input)
                .with_context(|| format!("{} must be a plugin fixture JSON", path.display()))?;
            Ok(LoadedPackageFixture { path, fixture })
        })
        .collect()
}

pub fn registry_import(input: RegistryImportInput) -> anyhow::Result<()> {
    let package_input = fs::read_to_string(&input.package_path)
        .with_context(|| format!("read {}", input.package_path))?;
    let package: ManifestPackage = serde_json::from_str(&package_input)
        .with_context(|| format!("{} must be a plugin package JSON", input.package_path))?;
    validate_package(
        &package,
        input.verify,
        input.allow_unsigned,
        &input.base_url,
    )?;
    let namespace = sanitize_namespace(&input.private_namespace)?;
    let package_dir = PathBuf::from(&input.registry_root)
        .join("private")
        .join(namespace)
        .join(&package.registry.id)
        .join(&package.registry.version);
    fs::create_dir_all(&package_dir)
        .with_context(|| format!("create {}", package_dir.display()))?;
    let manifest_rel = validate_relative_manifest_path(&package.registry.manifest_path)?;
    let manifest_path = package_dir.join(manifest_rel);
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&package.manifest)?,
    )
    .with_context(|| format!("write {}", manifest_path.display()))?;
    fs::write(package_dir.join("README.md"), package.readme)
        .with_context(|| format!("write {}", package_dir.join("README.md").display()))?;
    fs::write(package_dir.join("security.md"), package.security)
        .with_context(|| format!("write {}", package_dir.join("security.md").display()))?;
    if !package.fixtures.is_empty() {
        let fixtures_dir = package_dir.join("fixtures");
        fs::create_dir_all(&fixtures_dir)
            .with_context(|| format!("create {}", fixtures_dir.display()))?;
        for (idx, fixture) in package.fixtures.iter().enumerate() {
            fs::write(
                fixtures_dir.join(format!("fixture-{idx:03}.json")),
                serde_json::to_string_pretty(fixture)?,
            )
            .with_context(|| format!("write fixture {idx}"))?;
        }
    }
    let mut registry = load_registry_from_root(&input.registry_root)
        .unwrap_or_else(|_| empty_registry("Kooix Gate private manifest registry".to_string()));
    let mut entry = package.registry;
    entry.source = RegistrySource::Private;
    entry.manifest_path = path_for_registry(&manifest_path, &input.registry_root);
    upsert_registry_entry(&mut registry, entry);
    write_registry(&input.registry_root, &registry)?;
    println!("plugin registry import ok");
    Ok(())
}

pub fn registry_export(input: RegistryExportInput) -> anyhow::Result<()> {
    let mut registry = load_registry_from_root(&input.registry_root)?;
    if !input.include_private {
        registry
            .entries
            .retain(|e| e.source != RegistrySource::Private);
    }
    write_output(input.output, &serde_json::to_string_pretty(&registry)?)?;
    Ok(())
}

pub async fn test_connection(
    manifest_path: Option<String>,
    base_url: String,
    api_key: String,
    model: String,
    prompt: String,
    max_tokens: u32,
    timeout_ms: u64,
) -> anyhow::Result<()> {
    let value = parse_manifest(read_manifest(manifest_path)?)?;
    let provider = CustomHttpProvider::new_with_opts(
        &base_url,
        api_key,
        value,
        gate_providers::ProviderOpts { timeout_ms },
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let req = ChatRequest {
        model,
        messages: vec![ChatMessage::text(Role::User, prompt)],
        max_tokens: Some(max_tokens),
        temperature: Some(0.0),
        stream: false,
        ..Default::default()
    };
    let response = provider
        .chat(req)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

pub fn export_fixture(
    manifest_path: Option<String>,
    sse_path: Option<String>,
    response_sample_path: Option<String>,
    output: Option<String>,
    base_url: String,
    model: String,
) -> anyhow::Result<()> {
    let manifest = parse_manifest(read_manifest(manifest_path)?)?;
    gate_providers::validate_plugin_manifest(manifest.clone(), &base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let response_sample = response_sample_path
        .as_deref()
        .map(read_json_file)
        .transpose()?;
    let raw_sse = sse_path
        .as_deref()
        .map(|path| fs::read_to_string(path).with_context(|| format!("read {path}")))
        .transpose()?;
    let expected_chunks = raw_sse
        .as_ref()
        .map(|raw| {
            gate_providers::replay_plugin_sse(manifest.clone(), &base_url, raw.as_bytes(), &model)
                .map_err(|e| anyhow::anyhow!(e.to_string()))
        })
        .transpose()?;

    let fixture = PluginFixture {
        version: 1,
        base_url,
        model,
        manifest,
        response_sample,
        raw_sse,
        expected_chunks,
    };
    write_output(output, &serde_json::to_string_pretty(&fixture)?)?;
    Ok(())
}

pub fn import_fixture(
    fixture_path: Option<String>,
    verify: bool,
    output: Option<String>,
) -> anyhow::Result<()> {
    let input = read_named_input(fixture_path, "fixture")?;
    let fixture: PluginFixture =
        serde_json::from_str(&input).context("plugin fixture must be valid JSON")?;
    if fixture.version != 1 {
        bail!("unsupported plugin fixture version: {}", fixture.version);
    }
    gate_providers::validate_plugin_manifest(fixture.manifest.clone(), &fixture.base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    if verify {
        match (&fixture.raw_sse, &fixture.expected_chunks) {
            (Some(raw), Some(expected)) => {
                let actual = gate_providers::replay_plugin_sse(
                    fixture.manifest.clone(),
                    &fixture.base_url,
                    raw.as_bytes(),
                    &fixture.model,
                )
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let actual_json = normalize_replay_chunks(serde_json::to_value(&actual)?);
                let expected_json = normalize_replay_chunks(serde_json::to_value(expected)?);
                if actual_json != expected_json {
                    bail!(
                        "plugin fixture replay mismatch: expected {} chunks, got {} chunks",
                        expected.len(),
                        actual.len()
                    );
                }
            }
            (None, _) => bail!("fixture has no raw_sse to verify"),
            (_, None) => bail!("fixture has no expected_chunks to verify"),
        }
    }

    if output.is_some() {
        write_output(output, &serde_json::to_string_pretty(&fixture.manifest)?)?;
    } else {
        println!("plugin fixture ok");
    }
    Ok(())
}

fn parse_manifest(input: String) -> anyhow::Result<serde_json::Value> {
    serde_json::from_str(&input).context("plugin manifest must be valid JSON")
}

fn load_registry_from_root(root: &str) -> anyhow::Result<ManifestRegistry> {
    let path = Path::new(root).join("registry.json");
    let input = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut registry: ManifestRegistry =
        serde_json::from_str(&input).with_context(|| format!("{} must be JSON", path.display()))?;
    if registry.schema_version != REGISTRY_SCHEMA_VERSION {
        bail!(
            "unsupported registry schema_version: {}",
            registry.schema_version
        );
    }
    for entry in &mut registry.entries {
        validate_registry_entry(entry, root)?;
    }
    sort_registry_entries(&mut registry.entries);
    Ok(registry)
}

fn write_registry(root: &str, registry: &ManifestRegistry) -> anyhow::Result<()> {
    fs::create_dir_all(root).with_context(|| format!("create {root}"))?;
    let path = Path::new(root).join("registry.json");
    fs::write(&path, serde_json::to_string_pretty(registry)?)
        .with_context(|| format!("write {}", path.display()))
}

fn empty_registry(name: String) -> ManifestRegistry {
    ManifestRegistry {
        schema_version: REGISTRY_SCHEMA_VERSION,
        name,
        description: Some("Private HTTP Plugin manifest registry generated by kgctl".to_string()),
        entries: Vec::new(),
    }
}

fn validate_registry_entry(entry: &mut RegistryEntry, root: &str) -> anyhow::Result<()> {
    validate_registry_id(&entry.id)?;
    validate_version(&entry.version, "version")?;
    validate_compatibility(&entry.compatibility, &entry.id)?;
    validate_signature(&entry.signature, &entry.id, true)?;
    let manifest_rel = validate_relative_manifest_path(&entry.manifest_path)?;
    let manifest_path = Path::new(root).join(manifest_rel);
    let manifest_input = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest = parse_manifest(manifest_input)?;
    let actual_sha = manifest_digest(&manifest)?;
    if entry.sha256 != actual_sha {
        bail!(
            "registry entry {} sha256 mismatch: expected {}, got {}",
            entry.id,
            entry.sha256,
            actual_sha
        );
    }
    gate_providers::validate_plugin_manifest(manifest, "https://registry-validation.example/v1")
        .map_err(|e| anyhow::anyhow!("registry entry {} invalid manifest: {e}", entry.id))?;
    Ok(())
}

fn validate_compatibility(
    compatibility: &RegistryCompatibility,
    entry_id: &str,
) -> anyhow::Result<()> {
    validate_version(&compatibility.min_gate_version, "min_gate_version")?;
    if let Some(max) = compatibility.max_gate_version.as_deref() {
        validate_version(max, "max_gate_version")?;
    }
    if compatibility.manifest_schema != 1 {
        bail!(
            "registry entry {} uses unsupported manifest_schema {}",
            entry_id,
            compatibility.manifest_schema
        );
    }
    Ok(())
}

fn validate_signature(
    signature: &RegistrySignature,
    entry_id: &str,
    allow_unsigned: bool,
) -> anyhow::Result<()> {
    match signature.kind {
        SignatureKind::Unsigned if !allow_unsigned => {
            bail!("unsigned package requires --allow-unsigned")
        }
        SignatureKind::Unsigned => Ok(()),
        _ if signature.value.as_deref().is_none_or(str::is_empty) => {
            bail!("registry entry {entry_id} has signature kind without value")
        }
        SignatureKind::Minisign => verify_minisign_format(signature, entry_id),
        SignatureKind::Cosign | SignatureKind::SigstoreBundle => {
            // 0.4.54: schema 校验通过；真实 cosign sigstore-rs 链接留 0.5.0+
            // value 必须是合法 base64
            use base64::Engine as _;
            let v = signature.value.as_deref().unwrap();
            base64::engine::general_purpose::STANDARD
                .decode(v.trim())
                .map_err(|e| anyhow::anyhow!("registry entry {entry_id} signature.value not valid base64: {e}"))?;
            Ok(())
        }
    }
}

/// 0.4.54: minisign signature 格式校验（不做真实公钥验签——
/// 用户需提供 keypair 后由 0.5.0 lib 验签；本版仅 base64 + 长度检查）。
fn verify_minisign_format(
    signature: &RegistrySignature,
    entry_id: &str,
) -> anyhow::Result<()> {
    use base64::Engine as _;
    let v = signature.value.as_deref().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(v.trim())
        .map_err(|e| anyhow::anyhow!("registry entry {entry_id} minisign value not valid base64: {e}"))?;
    // minisign sig 二进制最小约 100B；过短即 reject
    if decoded.len() < 64 {
        bail!("registry entry {entry_id} minisign value too short ({} bytes < 64)", decoded.len());
    }
    // 如果提供 key_id，也需 base64
    if let Some(key) = signature.key_id.as_deref() {
        base64::engine::general_purpose::STANDARD
            .decode(key.trim())
            .map_err(|e| anyhow::anyhow!("registry entry {entry_id} minisign key_id not valid base64: {e}"))?;
    }
    Ok(())
}

fn validate_relative_manifest_path(path: &str) -> anyhow::Result<&Path> {
    validate_named_relative_path(path, "manifest.json")
}

fn validate_named_relative_path<'a>(path: &'a str, file_name: &str) -> anyhow::Result<&'a Path> {
    let rel = Path::new(path);
    if rel.as_os_str().is_empty() || rel.is_absolute() {
        bail!("{file_name} path must be a non-empty relative file path");
    }
    if !matches!(
        rel.file_name().and_then(|name| name.to_str()),
        Some(name) if name == file_name
    ) {
        bail!("{file_name} path must point to {file_name}");
    }
    let clean = rel
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if !clean {
        bail!("{file_name} path must not contain prefixes, roots, or '..'");
    }
    Ok(rel)
}

fn validate_relative_dir_path<'a>(path: &'a str, dir_name: &str) -> anyhow::Result<&'a Path> {
    let rel = Path::new(path);
    if rel.as_os_str().is_empty() || rel.is_absolute() {
        bail!("{dir_name} path must be a non-empty relative directory path");
    }
    if !matches!(
        rel.file_name().and_then(|name| name.to_str()),
        Some(name) if name == dir_name
    ) {
        bail!("{dir_name} path must point to {dir_name}/");
    }
    let clean = rel
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if !clean {
        bail!("{dir_name} path must not contain prefixes, roots, or '..'");
    }
    Ok(rel)
}

fn validate_package(
    package: &ManifestPackage,
    verify: bool,
    allow_unsigned: bool,
    base_url: &str,
) -> anyhow::Result<()> {
    if package.schema_version != PACKAGE_SCHEMA_VERSION {
        bail!(
            "unsupported plugin package schema_version: {}",
            package.schema_version
        );
    }
    let entry = &package.registry;
    validate_registry_id(&entry.id)?;
    validate_version(&entry.version, "version")?;
    validate_compatibility(&entry.compatibility, &entry.id)?;
    validate_signature(&entry.signature, &entry.id, allow_unsigned)?;
    validate_relative_manifest_path(&entry.manifest_path)?;
    let actual_sha = manifest_digest(&package.manifest)?;
    if entry.sha256 != actual_sha {
        bail!(
            "package sha256 mismatch: expected {}, got {}",
            entry.sha256,
            actual_sha
        );
    }
    gate_providers::validate_plugin_manifest(package.manifest.clone(), base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if verify {
        for fixture in &package.fixtures {
            validate_fixture(fixture)?;
            verify_fixture_replay(fixture)
                .with_context(|| format!("verify package fixture for {}", entry.id))?;
        }
    }
    Ok(())
}

fn validate_fixture(fixture: &PluginFixture) -> anyhow::Result<()> {
    if fixture.version != 1 {
        bail!("unsupported plugin fixture version: {}", fixture.version);
    }
    gate_providers::validate_plugin_manifest(fixture.manifest.clone(), &fixture.base_url)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

fn verify_fixture_replay(fixture: &PluginFixture) -> anyhow::Result<()> {
    let Some(raw) = &fixture.raw_sse else {
        bail!("fixture has no raw_sse to verify");
    };
    let Some(expected) = &fixture.expected_chunks else {
        bail!("fixture has no expected_chunks to verify");
    };
    let actual = gate_providers::replay_plugin_sse(
        fixture.manifest.clone(),
        &fixture.base_url,
        raw.as_bytes(),
        &fixture.model,
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let actual_json = normalize_replay_chunks(serde_json::to_value(&actual)?);
    let expected_json = normalize_replay_chunks(serde_json::to_value(expected)?);
    if actual_json != expected_json {
        bail!(
            "plugin fixture replay mismatch: expected {} chunks, got {} chunks",
            expected.len(),
            actual.len()
        );
    }
    Ok(())
}

fn ensure_markdown_contains(path: &Path, needle: &str, label: &str) -> anyhow::Result<()> {
    let content = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if !content.to_lowercase().contains(&needle.to_lowercase()) {
        bail!("{label} must mention {needle}");
    }
    Ok(())
}

fn upsert_registry_entry(registry: &mut ManifestRegistry, entry: RegistryEntry) {
    if let Some(existing) = registry
        .entries
        .iter_mut()
        .find(|e| e.id == entry.id && e.version == entry.version)
    {
        *existing = entry;
    } else {
        registry.entries.push(entry);
    }
    sort_registry_entries(&mut registry.entries);
}

fn sort_registry_entries(entries: &mut [RegistryEntry]) {
    entries.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.version.cmp(&b.version)));
}

fn validate_registry_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
    {
        bail!("registry id must use lowercase [a-z0-9_-] and be <= 96 chars");
    }
    Ok(())
}

fn validate_version(value: &str, field: &str) -> anyhow::Result<()> {
    let mut parts = value.split('.');
    let ok = (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    }) && parts.next().is_none();
    if !ok {
        bail!("{field} must use semver-like MAJOR.MINOR.PATCH");
    }
    Ok(())
}

fn parse_source(value: &str) -> anyhow::Result<RegistrySource> {
    match value {
        "official" => Ok(RegistrySource::Official),
        "community" => Ok(RegistrySource::Community),
        "private" => Ok(RegistrySource::Private),
        other => bail!("source must be official, community, or private; got {other}"),
    }
}

fn parse_signature_kind(value: &str) -> anyhow::Result<SignatureKind> {
    match value {
        "unsigned" => Ok(SignatureKind::Unsigned),
        "cosign" => Ok(SignatureKind::Cosign),
        "minisign" => Ok(SignatureKind::Minisign),
        "sigstore_bundle" => Ok(SignatureKind::SigstoreBundle),
        other => bail!(
            "signature kind must be unsigned, cosign, minisign, or sigstore_bundle; got {other}"
        ),
    }
}

fn source_str(source: &RegistrySource) -> &'static str {
    match source {
        RegistrySource::Official => "official",
        RegistrySource::Community => "community",
        RegistrySource::Private => "private",
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn manifest_digest(manifest: &serde_json::Value) -> anyhow::Result<String> {
    Ok(sha256_hex(
        serde_json::to_string(&sort_json(manifest.clone()))?.as_bytes(),
    ))
}

fn sort_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (key, value) in entries {
                sorted.insert(key, sort_json(value));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(sort_json).collect())
        }
        other => other,
    }
}

fn default_package_readme(name: &str, id: &str) -> String {
    format!("# {name}\n\nManifest package `{id}` generated by `kgctl plugin registry package`.\n")
}

fn default_package_security() -> String {
    "# Security\n\nReview outbound URLs, secret slots, headers, retry policy and fixture samples before importing this package.\n".to_string()
}

fn sanitize_namespace(ns: &str) -> anyhow::Result<String> {
    validate_registry_id(ns)?;
    Ok(ns.to_string())
}

fn path_for_registry(path: &Path, root: &str) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn read_manifest(path: Option<String>) -> anyhow::Result<String> {
    read_named_input(path, "manifest")
}

fn read_named_input(path: Option<String>, kind: &str) -> anyhow::Result<String> {
    match path.as_deref() {
        Some("-") | None => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .with_context(|| format!("read {kind} from stdin"))?;
            if buf.trim().is_empty() {
                bail!("plugin {kind} input is empty");
            }
            Ok(buf)
        }
        Some(path) => fs::read_to_string(path).with_context(|| format!("read {path}")),
    }
}

fn read_json_file(path: &str) -> anyhow::Result<serde_json::Value> {
    let input = fs::read_to_string(path).with_context(|| format!("read {path}"))?;
    serde_json::from_str(&input).with_context(|| format!("{path} must be valid JSON"))
}

fn write_output(path: Option<String>, content: &str) -> anyhow::Result<()> {
    match path.as_deref() {
        Some("-") | None => {
            println!("{content}");
            Ok(())
        }
        Some(path) => fs::write(path, content).with_context(|| format!("write {path}")),
    }
}

fn normalize_replay_chunks(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(chunks) = value.as_array_mut() {
        for chunk in chunks {
            if let Some(obj) = chunk.as_object_mut()
                && obj
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id.starts_with("chatcmpl-"))
            {
                obj.insert(
                    "id".to_string(),
                    serde_json::Value::String("<generated-chatcmpl-id>".to_string()),
                );
            }
        }
    }
    value
}
