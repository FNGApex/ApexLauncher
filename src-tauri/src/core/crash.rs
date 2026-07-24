//! Pure crash-report parser (CP-1 of the crash-log-help spec).
//!
//! `parse_crash_report` turns the raw text of a Minecraft `mc/crash-reports/*.txt` file
//! into a [`ParsedReport`]. Everything here is a pure `&str -> struct` transform: no
//! Tauri types, no `regex` crate, no network, no I/O. Malformed, truncated, or empty
//! input never panics — fields simply come back `None`/empty.
//!
//! Downstream checkpoints build on this shape:
//! - CP-2 (`analyze`) turns a [`ParsedReport`] + log tail into a [`CrashAnalysis`].
//! - CP-3 (`resolve_suspects`) cross-references [`ParsedReport::mods`] and the
//!   suspect vectors against the instance's mod manifest (`ModEntry`/`FolderMod`,
//!   imported from the sibling `instances` module — not a Tauri type, so the
//!   "no Tauri types" rule above still holds).

use std::collections::HashSet;

use crate::core::instances::{FolderMod, ModEntry};

/// Frame package-prefix exclusion list for the suspect-package fallback (spec §Attribution).
/// A frame's package is skipped as a suspect candidate if it starts with any of these.
const EXCLUDED_PACKAGE_PREFIXES: &[&str] = &[
    "java.",
    "javax.",
    "jdk.",
    "sun.",
    "com.sun.",
    "net.minecraft.",
    "com.mojang.",
    "net.fabricmc.",
    "org.quiltmc.",
    "net.minecraftforge.",
    "net.neoforged.",
    "cpw.mods.",
    "org.spongepowered.",
    "org.lwjgl.",
    "io.netty.",
    "com.google.",
    "org.apache.",
    "org.slf4j.",
    "it.unimi.",
    "org.joml.",
];

/// Vanilla jar-name prefixes filtered out of [`ParsedReport::suspect_jars`].
const VANILLA_JAR_PREFIXES: &[&str] = &["client-", "server-", "minecraft-"];

/// Maximum stack frames retained on [`ExceptionInfo::frames`].
const MAX_FRAMES: usize = 15;

/// Maximum suspect packages surfaced via the fallback (spec §Attribution: "first 2").
const MAX_SUSPECT_PACKAGES: usize = 2;

/// The parsed shape of a Minecraft crash-report text file (spec §Data shapes).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedReport {
    /// The `Description:` line (freeform, e.g. `"Rendering overlay"`).
    pub description: Option<String>,
    /// The `Time:` line, verbatim (report-local format, not reparsed).
    pub time: Option<String>,
    /// The top-level exception: class, optional message, and capped stack frames.
    pub exception: Option<ExceptionInfo>,
    /// `Minecraft Version:` from the `-- System Details --` block.
    pub minecraft_version: Option<String>,
    /// Suspect mod ids from `TRANSFORMER/<id>@` and `{mixin from mod <id>}` frame annotations.
    pub suspect_mod_ids: Vec<String>,
    /// Suspect jars from `~[<jar>…]` frame annotations, vanilla jars filtered out.
    pub suspect_jars: Vec<String>,
    /// First non-excluded frame packages; always populated — CP-3 ranks these last,
    /// it does not gate on ids/jars being empty here.
    pub suspect_packages: Vec<String>,
    /// Union of the `Fabric Mods:` table and the `Mod List:` pipe table.
    pub mods: Vec<ReportMod>,
}

/// The report's top-level exception.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionInfo {
    /// Fully-qualified exception class name (e.g. `"java.lang.OutOfMemoryError"`).
    pub class: String,
    /// The message after `": "`, if present.
    pub message: Option<String>,
    /// Stack frames immediately following the exception header, capped at [`MAX_FRAMES`].
    pub frames: Vec<String>,
}

/// One mod entry from either the `Fabric Mods:` table or the `Mod List:` pipe table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportMod {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Only populated by the `Mod List:` (Forge/NeoForge) table; `None` for Fabric.
    pub jar: Option<String>,
}

/// Parse the text content of a Minecraft crash report.
///
/// Always succeeds. Any section that cannot be located or looks malformed is left
/// `None`/empty rather than causing a panic or an `Err`.
pub fn parse_crash_report(text: &str) -> ParsedReport {
    let lines: Vec<&str> = text.lines().collect();

    let description = extract_labeled_line(&lines, "Description:");
    let time = extract_labeled_line(&lines, "Time:");
    let minecraft_version = extract_labeled_line(&lines, "Minecraft Version:");

    let exception = extract_exception(&lines);

    let mut mods = extract_fabric_mods(&lines);
    mods.extend(extract_mod_list(&lines));

    let all_frames = collect_all_frame_lines(&lines);
    let suspect_mod_ids = extract_suspect_mod_ids(&all_frames);
    let suspect_jars = extract_suspect_jars(&all_frames);
    let suspect_packages = extract_suspect_packages(&all_frames);

    ParsedReport {
        description,
        time,
        exception,
        minecraft_version,
        suspect_mod_ids,
        suspect_jars,
        suspect_packages,
        mods,
    }
}

// ── Description / Time / Minecraft Version ────────────────────────────────────

/// Return the trimmed remainder of the first line whose trimmed content starts with
/// `label` (label includes the trailing `:`).
///
/// `"Minecraft Version:"` deliberately does not match `"Minecraft Version ID:"` since
/// the colon position differs (`strip_prefix` requires an exact prefix match).
fn extract_labeled_line(lines: &[&str], label: &str) -> Option<String> {
    for line in lines {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(label) {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

// ── Exception + frames ─────────────────────────────────────────────────────────

fn extract_exception(lines: &[&str]) -> Option<ExceptionInfo> {
    let desc_idx = lines.iter().position(|l| l.trim().starts_with("Description:"));
    let start = desc_idx.map(|i| i + 1).unwrap_or(0);

    let mut idx = start;
    let header_idx = loop {
        if idx >= lines.len() {
            return None;
        }
        let raw = lines[idx];
        let trimmed = raw.trim();
        if trimmed.is_empty() || is_skippable_header_line(raw, trimmed) {
            idx += 1;
            continue;
        }
        break idx;
    };

    let header = lines[header_idx].trim();
    let (class, message) = split_exception_header(header)?;

    let mut frames = Vec::new();
    let mut i = header_idx + 1;
    while i < lines.len() {
        match lines[i].trim_start().strip_prefix("at ") {
            Some(frame) => {
                if frames.len() < MAX_FRAMES {
                    frames.push(frame.trim_end().to_string());
                }
                i += 1;
            }
            None => break,
        }
    }

    Some(ExceptionInfo { class, message, frames })
}

/// `true` for lines that cannot be the exception header: indented lines (frames/details),
/// comment lines, section dividers, and the known `Time:`/`Description:` label lines.
fn is_skippable_header_line(raw: &str, trimmed: &str) -> bool {
    if raw.starts_with(' ') || raw.starts_with('\t') {
        return true;
    }
    trimmed.starts_with("//")
        || trimmed.starts_with("----")
        || trimmed.starts_with("--")
        || trimmed.starts_with("Time:")
        || trimmed.starts_with("Description:")
        || trimmed.starts_with("A detailed walkthrough")
}

/// Split `"com.example.FooException: message"` into `("com.example.FooException",
/// Some("message"))`, or a bare `"com.example.FooException"` into `(class, None)`.
/// Returns `None` if `header` does not look like a plausible exception class name.
fn split_exception_header(header: &str) -> Option<(String, Option<String>)> {
    if let Some(colon_pos) = header.find(": ") {
        let class = &header[..colon_pos];
        if !is_plausible_class_name(class) {
            return None;
        }
        let message = header[colon_pos + 2..].trim();
        return Some((
            class.to_string(),
            if message.is_empty() { None } else { Some(message.to_string()) },
        ));
    }
    if is_plausible_class_name(header) {
        return Some((header.to_string(), None));
    }
    None
}

/// A plausible fully-qualified exception class name: non-empty, no internal
/// whitespace, at least one `.` (package separator), and only identifier-safe chars.
fn is_plausible_class_name(s: &str) -> bool {
    !s.is_empty()
        && s.contains('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '$')
}

/// Collect the content of every `"at …"` frame line in the whole report (both the
/// exception's own trace and any `-- Head --` stacktrace repeat), for suspect scanning.
/// Not capped — capping only applies to [`ExceptionInfo::frames`].
fn collect_all_frame_lines(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|l| l.trim_start().strip_prefix("at ").map(|s| s.trim_end().to_string()))
        .collect()
}

// ── Fabric Mods: table ─────────────────────────────────────────────────────────

/// Parse the `Fabric Mods:` section: `\tmodid: Name version` lines, one mod per line.
/// The last whitespace-separated token is taken as the version; everything before it
/// (trimmed) is the name.
fn extract_fabric_mods(lines: &[&str]) -> Vec<ReportMod> {
    let mut mods = Vec::new();
    let mut in_section = false;

    for line in lines {
        if !in_section {
            if line.trim() == "Fabric Mods:" {
                in_section = true;
            }
            continue;
        }
        if line.trim().is_empty() || !(line.starts_with('\t') || line.starts_with(' ')) {
            break;
        }

        let entry = line.trim();
        if let Some((id, rest)) = entry.split_once(':') {
            let id = id.trim();
            let rest = rest.trim();
            if id.is_empty() || rest.is_empty() {
                continue;
            }
            let (name, version) = split_name_version(rest);
            mods.push(ReportMod { id: id.to_string(), name, version, jar: None });
        }
    }

    mods
}

/// Split `"Fabric Loader 0.15.7"` into `("Fabric Loader", "0.15.7")` by taking the
/// last whitespace-separated token as the version.
fn split_name_version(rest: &str) -> (String, String) {
    match rest.rsplit_once(' ') {
        Some((name, version)) if !name.trim().is_empty() && !version.trim().is_empty() => {
            (name.trim().to_string(), version.trim().to_string())
        }
        _ => (rest.to_string(), String::new()),
    }
}

// ── Mod List: pipe table (Forge/NeoForge) ──────────────────────────────────────

/// Parse the `Mod List:` pipe-column table. Column order is `jar | name | modid | version`
/// (spec §CP-1 test list). Rows with fewer than 4 columns, or an empty modid column,
/// are skipped rather than causing a failure.
fn extract_mod_list(lines: &[&str]) -> Vec<ReportMod> {
    let mut mods = Vec::new();
    let mut in_section = false;

    for line in lines {
        if !in_section {
            if line.trim() == "Mod List:" {
                in_section = true;
            }
            continue;
        }
        if line.trim().is_empty() || !(line.starts_with('\t') || line.starts_with(' ')) {
            break;
        }

        let entry = line.trim();
        if !entry.contains('|') {
            continue;
        }
        let cols: Vec<&str> = entry.split('|').map(|c| c.trim()).collect();
        if cols.len() < 4 {
            continue;
        }
        let (jar, name, id, version) = (cols[0], cols[1], cols[2], cols[3]);
        if id.is_empty() {
            continue;
        }
        mods.push(ReportMod {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            jar: if jar.is_empty() { None } else { Some(jar.to_string()) },
        });
    }

    mods
}

// ── Suspect attribution (spec §Attribution) ────────────────────────────────────

/// Suspect ids from `TRANSFORMER/<id>@` and `{mixin from mod <id>}`/`{mixin from mod
/// <id>: <cfg>}` frame annotations, deduplicated, in order of first appearance.
fn extract_suspect_mod_ids(frames: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for frame in frames {
        if let Some(id) = extract_between(frame, "TRANSFORMER/", '@') {
            push_unique(&mut result, &mut seen, id);
        }
        if let Some(pos) = frame.find("{mixin from mod ") {
            let rest = &frame[pos + "{mixin from mod ".len()..];
            let end = rest.find([':', '}']).unwrap_or(rest.len());
            let id = rest[..end].trim();
            if !id.is_empty() {
                push_unique(&mut result, &mut seen, id.to_string());
            }
        }
    }

    result
}

/// Return the substring of `s` between the first occurrence of `prefix` and the
/// following `terminator` char, or `None` if either is absent or the span is empty.
fn extract_between(s: &str, prefix: &str, terminator: char) -> Option<String> {
    let start = s.find(prefix)? + prefix.len();
    let rest = &s[start..];
    let end = rest.find(terminator)?;
    let id = &rest[..end];
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Suspect jars from `~[<jar>…]` frame annotations.
///
/// The jar name is taken up to the first of `%`, `!`, or `:` (whichever comes first;
/// `:` handles the common `~[modjar.jar:?]`/`~[?:?]` form where a colon separates the
/// jar filename from a module-version qualifier, and `%`/`!` handle the nested/shaded
/// jar-in-jar form `~[modjar.jar%23392!/:?]` — matches spec's stated `%`/`!` delimiters
/// while also making the exact `"?"` vanilla-filter entry meaningful for `~[?:?]`
/// bootstrap-class frames). Vanilla jars (`?`, `client-*`, `server-*`, `minecraft-*`)
/// are filtered out. Deduplicated, in order of first appearance.
fn extract_suspect_jars(frames: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for frame in frames {
        let Some(start) = frame.find("~[") else { continue };
        let rest = &frame[start + 2..];
        let end = rest.find(['%', '!', ':', ']']).unwrap_or(rest.len());
        let jar = rest[..end].trim();

        if jar.is_empty() || jar == "?" {
            continue;
        }
        if VANILLA_JAR_PREFIXES.iter().any(|p| jar.starts_with(p)) {
            continue;
        }
        if seen.insert(jar.to_string()) {
            result.push(jar.to_string());
        }
    }

    result
}

/// Fallback: the first [`MAX_SUSPECT_PACKAGES`] non-excluded frame packages, in order
/// of first appearance, deduplicated.
fn extract_suspect_packages(frames: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for frame in frames {
        if result.len() >= MAX_SUSPECT_PACKAGES {
            break;
        }
        let Some(pkg) = frame_package(frame) else { continue };
        if is_excluded_package(&pkg) {
            continue;
        }
        if seen.insert(pkg.clone()) {
            result.push(pkg);
        }
    }

    result
}

/// Extract the package of a stack-frame line's class (dropping any `TRANSFORMER/<id>@
/// <ver>/` prefix, the method name, and the class name itself).
fn frame_package(frame: &str) -> Option<String> {
    let after_transformer = match frame.find("TRANSFORMER/") {
        Some(pos) => {
            let rest = &frame[pos + "TRANSFORMER/".len()..];
            let slash = rest.find('/')?;
            &rest[slash + 1..]
        }
        None => frame,
    };

    let paren = after_transformer.find('(')?;
    let fqcn_and_method = &after_transformer[..paren];
    let method_dot = fqcn_and_method.rfind('.')?;
    let fqcn = &fqcn_and_method[..method_dot];
    let class_dot = fqcn.rfind('.')?;
    Some(fqcn[..class_dot].to_string())
}

/// `true` if `pkg` should be skipped as a package-fallback suspect candidate: either it
/// sits under one of [`EXCLUDED_PACKAGE_PREFIXES`] (`pkg.starts_with(prefix)`), or `pkg`
/// itself IS the excluded package with no trailing dot (e.g. a two-segment frame class
/// like `net.minecraft.class_310` yields the bare package `"net.minecraft"`, which must
/// match the `"net.minecraft."` entry even though the trailing dot itself is absent).
fn is_excluded_package(pkg: &str) -> bool {
    EXCLUDED_PACKAGE_PREFIXES
        .iter()
        .any(|prefix| pkg.starts_with(prefix) || pkg == prefix.trim_end_matches('.'))
}

fn push_unique(result: &mut Vec<String>, seen: &mut HashSet<String>, val: String) {
    if seen.insert(val.clone()) {
        result.push(val);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CP-2: rule engine (`analyze`)
// ═══════════════════════════════════════════════════════════════════════════════

/// Input to [`analyze`] (spec §Data shapes). Pulled together at the call site from a
/// (possibly absent) parsed crash report, the in-memory log-ring tail, and process
/// exit info — no I/O happens here.
pub struct AnalyzeInput<'a> {
    pub report: Option<&'a ParsedReport>,
    pub report_text: Option<&'a str>,
    /// Last ≤300 ring lines, stream-agnostic.
    pub log_tail: &'a [String],
    pub exit_code: Option<i32>,
    pub report_path: Option<String>,
    /// `hs_err_pid*.log` path, if present.
    pub jvm_error_path: Option<String>,
}

/// One suspected mod surfaced alongside a [`CrashAnalysis`]. Populated by CP-3's
/// `resolve_suspects` — `analyze` always leaves [`CrashAnalysis::suspects`] empty.
#[derive(Clone)]
pub struct CrashSuspect {
    pub display: String,
    pub mod_id: Option<String>,
    pub jar: Option<String>,
}

/// The result of running the ordered `RULES` table against an [`AnalyzeInput`].
#[derive(Clone)]
pub struct CrashAnalysis {
    /// Stable rule id (spec §Rule table) — part of the IPC contract, never renamed.
    pub kind: String,
    pub headline: String,
    pub suggestion: String,
    /// `"class: message"` single line, built from `report.exception` when present.
    pub exception: Option<String>,
    /// Always empty as of CP-2 — populated by CP-3's `resolve_suspects`.
    pub suspects: Vec<CrashSuspect>,
    /// Verbatim key lines pulled by the matched rule's detail extractor, capped at 12.
    pub detail: Vec<String>,
    pub report_path: Option<String>,
    pub jvm_error_path: Option<String>,
}

/// The un-enriched shape a single rule function hands back to [`analyze`]; `analyze`
/// fills in `exception`/`suspects`/`report_path`/`jvm_error_path` uniformly afterwards.
struct RuleMatch {
    kind: &'static str,
    headline: String,
    suggestion: String,
    detail: Vec<String>,
}

type RuleFn = fn(&AnalyzeInput, Option<&str>) -> Option<RuleMatch>;

/// Ordered rule table (spec §Rule table) — first match wins. `rule_generic` is last
/// and always matches, so `RULES.iter().find_map(...)` in [`analyze`] never falls through.
static RULES: &[RuleFn] = &[
    rule_fabric_unmet_deps,
    rule_forge_missing_deps,
    rule_duplicate_mods,
    rule_out_of_memory,
    rule_unsupported_java,
    rule_mixin_failure,
    rule_missing_class,
    rule_native_crash,
    rule_gl_error,
    rule_mod_crash,
    rule_generic,
];

/// Run the ordered `RULES` table over `input`, returning the first match (spec §Rule
/// table: "first match wins"). `rule_generic` always matches, so this never panics.
pub fn analyze(input: AnalyzeInput) -> CrashAnalysis {
    let exception_line = build_exception_line(input.report);

    let matched = RULES
        .iter()
        .find_map(|rule_fn| rule_fn(&input, exception_line.as_deref()))
        .expect("rule_generic always matches");

    CrashAnalysis {
        kind: matched.kind.to_string(),
        headline: matched.headline,
        suggestion: matched.suggestion,
        exception: exception_line,
        suspects: Vec::new(),
        detail: matched.detail,
        report_path: input.report_path,
        jvm_error_path: input.jvm_error_path,
    }
}

/// `CrashAnalysis.exception` = `"class: message"` (or bare `class` when there's no
/// message) from the report's top-level exception, when present.
fn build_exception_line(report: Option<&ParsedReport>) -> Option<String> {
    let exc = report?.exception.as_ref()?;
    Some(match &exc.message {
        Some(msg) => format!("{}: {}", exc.class, msg),
        None => exc.class.clone(),
    })
}

/// Fill the first `{}` slot in `template` with `val` (spec: suggestion/headline
/// strings are `&'static str` templates with `{}` slots + a small format helper).
fn fmt1(template: &str, val: &str) -> String {
    template.replacen("{}", val, 1)
}

// ── Needle matching (spec: exception line, then raw report text, then each log-tail line) ──

/// `true` if any of `needles` is a case-sensitive substring of the exception line,
/// the raw report text, or any log-tail line, checked in that order.
fn any_needle_matches(input: &AnalyzeInput, exception_line: Option<&str>, needles: &[&str]) -> bool {
    if let Some(exc) = exception_line {
        if needles.iter().any(|n| exc.contains(n)) {
            return true;
        }
    }
    if let Some(text) = input.report_text {
        if needles.iter().any(|n| text.contains(n)) {
            return true;
        }
    }
    input.log_tail.iter().any(|line| needles.iter().any(|n| line.contains(n)))
}

/// The first whole line (exception line, then a `report_text` line, then a
/// `log_tail` line, in that order) containing any of `needles`, trimmed.
fn line_matching_any_needle(input: &AnalyzeInput, exception_line: Option<&str>, needles: &[&str]) -> Option<String> {
    if let Some(exc) = exception_line {
        if needles.iter().any(|n| exc.contains(n)) {
            return Some(exc.trim().to_string());
        }
    }
    if let Some(text) = input.report_text {
        if let Some(line) = text.lines().find(|l| needles.iter().any(|n| l.contains(n))) {
            return Some(line.trim().to_string());
        }
    }
    input.log_tail.iter().find(|l| needles.iter().any(|n| l.contains(n))).map(|l| l.trim().to_string())
}

/// `report_text` lines followed by `log_tail` lines — the combined verbatim search
/// space used by detail extractors that scan for shaped lines (not just a single
/// matched line).
fn combined_lines<'a>(input: &'a AnalyzeInput) -> Vec<&'a str> {
    let mut lines: Vec<&str> = Vec::new();
    if let Some(text) = input.report_text {
        lines.extend(text.lines());
    }
    lines.extend(input.log_tail.iter().map(|s| s.as_str()));
    lines
}

/// Trimmed, non-blank lines immediately following the first line containing any of
/// `needles`, capped at `cap`, stopping at the first blank line (spec rules 2/3:
/// "verbatim block/lines … after the needle line").
fn lines_after_any_needle(lines: &[&str], needles: &[&str], cap: usize) -> Vec<String> {
    let Some(pos) = lines.iter().position(|l| needles.iter().any(|n| l.contains(n))) else {
        return Vec::new();
    };
    lines[pos + 1..]
        .iter()
        .take_while(|l| !l.trim().is_empty())
        .take(cap)
        .map(|l| l.trim().to_string())
        .collect()
}

/// Extract the substring immediately after `needle` in `line` (stripping a leading
/// `':'` and surrounding whitespace, then taking the first whitespace-delimited
/// token) — used to pull a bare class name out of an exception line.
fn extract_class_name_after(line: &str, needle: &str) -> Option<String> {
    let pos = line.find(needle)?;
    let rest = &line[pos + needle.len()..];
    let rest = rest.strip_prefix(':').unwrap_or(rest).trim();
    if rest.is_empty() {
        return None;
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

// ── Rule 1: fabric_unmet_deps ────────────────────────────────────────────────────

const FABRIC_UNMET_DEPS_NEEDLES: &[&str] = &[
    "Mod resolution encountered an incompatible mod set",
    "Incompatible mods found",
    "Unmet dependency listing",
];

fn rule_fabric_unmet_deps(input: &AnalyzeInput, exception_line: Option<&str>) -> Option<RuleMatch> {
    if !any_needle_matches(input, exception_line, FABRIC_UNMET_DEPS_NEEDLES) {
        return None;
    }
    let detail = combined_lines(input)
        .into_iter()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("- Mod '") || t.contains("requires version") || t.contains("which is missing")
        })
        .take(12)
        .map(|l| l.trim().to_string())
        .collect();
    Some(RuleMatch {
        kind: "fabric_unmet_deps",
        headline: "Missing or incompatible mod dependencies.".to_string(),
        suggestion: "Install or update the mods listed below to satisfy the missing dependency requirements."
            .to_string(),
        detail,
    })
}

// ── Rule 2: forge_missing_deps ───────────────────────────────────────────────────

const FORGE_MISSING_DEPS_NEEDLES: &[&str] = &["Missing or unsupported mandatory dependencies"];

fn rule_forge_missing_deps(input: &AnalyzeInput, exception_line: Option<&str>) -> Option<RuleMatch> {
    if !any_needle_matches(input, exception_line, FORGE_MISSING_DEPS_NEEDLES) {
        return None;
    }
    let lines = combined_lines(input);
    let detail = lines_after_any_needle(&lines, FORGE_MISSING_DEPS_NEEDLES, 12);
    Some(RuleMatch {
        kind: "forge_missing_deps",
        headline: "Missing required mod dependencies.".to_string(),
        suggestion: "Install the missing mods listed below, matching the required versions.".to_string(),
        detail,
    })
}

// ── Rule 3: duplicate_mods ───────────────────────────────────────────────────────

const DUPLICATE_MODS_NEEDLES: &[&str] =
    &["DuplicateModsFoundException", "Found duplicate mods", "duplicate mods"];

fn rule_duplicate_mods(input: &AnalyzeInput, exception_line: Option<&str>) -> Option<RuleMatch> {
    if !any_needle_matches(input, exception_line, DUPLICATE_MODS_NEEDLES) {
        return None;
    }
    let lines = combined_lines(input);
    let detail = lines_after_any_needle(&lines, DUPLICATE_MODS_NEEDLES, 12);
    Some(RuleMatch {
        kind: "duplicate_mods",
        headline: "The same mod is installed twice.".to_string(),
        suggestion: "Remove one of the duplicate jar files and relaunch.".to_string(),
        detail,
    })
}

// ── Rule 4: out_of_memory ────────────────────────────────────────────────────────

const OUT_OF_MEMORY_NEEDLES: &[&str] = &["java.lang.OutOfMemoryError"];

fn rule_out_of_memory(input: &AnalyzeInput, exception_line: Option<&str>) -> Option<RuleMatch> {
    let line = line_matching_any_needle(input, exception_line, OUT_OF_MEMORY_NEEDLES)?;
    Some(RuleMatch {
        kind: "out_of_memory",
        headline: "The game ran out of memory.".to_string(),
        suggestion: "Raise the allocated memory in the instance's Java settings and relaunch.".to_string(),
        detail: vec![line],
    })
}

// ── Rule 5: unsupported_java ─────────────────────────────────────────────────────

const UNSUPPORTED_JAVA_NEEDLES: &[&str] = &[
    "UnsupportedClassVersionError",
    "has been compiled by a more recent version of the Java Runtime",
];

fn rule_unsupported_java(input: &AnalyzeInput, exception_line: Option<&str>) -> Option<RuleMatch> {
    let line = line_matching_any_needle(input, exception_line, UNSUPPORTED_JAVA_NEEDLES)?;
    Some(RuleMatch {
        kind: "unsupported_java",
        headline: "This mod or Minecraft version needs a different Java version.".to_string(),
        suggestion: "Check the instance's Java settings and switch to the required Java major version."
            .to_string(),
        detail: vec![line],
    })
}

// ── Rule 6: mixin_failure ────────────────────────────────────────────────────────

const MIXIN_FAILURE_NEEDLES: &[&str] = &["MixinApplyError", "InjectionError", "Mixin apply failed"];

fn rule_mixin_failure(input: &AnalyzeInput, exception_line: Option<&str>) -> Option<RuleMatch> {
    let frame_match =
        input.report.and_then(|r| r.exception.as_ref()).and_then(|e| {
            e.frames.iter().find(|f| f.contains("mixin from mod")).cloned()
        });

    let detail = if let Some(frame) = frame_match {
        vec![frame]
    } else if any_needle_matches(input, exception_line, MIXIN_FAILURE_NEEDLES) {
        line_matching_any_needle(input, exception_line, MIXIN_FAILURE_NEEDLES).into_iter().collect()
    } else {
        return None;
    };

    Some(RuleMatch {
        kind: "mixin_failure",
        headline: "A mod's mixin failed to apply.".to_string(),
        suggestion: "This is usually a version incompatibility — try updating or removing the suspect mod."
            .to_string(),
        detail,
    })
}

// ── Rule 7: missing_class ────────────────────────────────────────────────────────

const MISSING_CLASS_NEEDLES: &[&str] = &["ClassNotFoundException", "NoClassDefFoundError"];

fn rule_missing_class(input: &AnalyzeInput, exception_line: Option<&str>) -> Option<RuleMatch> {
    let line = line_matching_any_needle(input, exception_line, MISSING_CLASS_NEEDLES)?;
    let class_name = MISSING_CLASS_NEEDLES.iter().find_map(|needle| extract_class_name_after(&line, needle));
    Some(RuleMatch {
        kind: "missing_class",
        headline: "A mod references a class that isn't present.".to_string(),
        suggestion: "This usually means a missing dependency or a version mismatch — update or reinstall the \
                     affected mods."
            .to_string(),
        detail: class_name.into_iter().collect(),
    })
}

// ── Rule 8: native_crash ─────────────────────────────────────────────────────────

/// Exit codes that indicate a JVM/native crash rather than a normal abnormal exit
/// (Windows access violation, Windows heap corruption, SIGABRT, SIGSEGV — POSIX
/// `128 + signal` convention).
const NATIVE_CRASH_EXIT_CODES: &[i32] = &[-1073741819, -1073740791, 134, 139];

fn rule_native_crash(input: &AnalyzeInput, _exception_line: Option<&str>) -> Option<RuleMatch> {
    let exit_matches = input.exit_code.map(|c| NATIVE_CRASH_EXIT_CODES.contains(&c)).unwrap_or(false);
    if !exit_matches && input.jvm_error_path.is_none() {
        return None;
    }
    Some(RuleMatch {
        kind: "native_crash",
        headline: "The JVM or native code crashed.".to_string(),
        suggestion: "Update your graphics drivers and Java runtime; the JVM error log has more detail."
            .to_string(),
        detail: Vec::new(),
    })
}

// ── Rule 9: gl_error ──────────────────────────────────────────────────────────────

const GL_ERROR_NEEDLES: &[&str] =
    &["GLFW error", "Failed to create GLFW window", "does not appear to support OpenGL"];

fn rule_gl_error(input: &AnalyzeInput, exception_line: Option<&str>) -> Option<RuleMatch> {
    let lwjgl_in_exception = exception_line.map(|e| e.contains("org.lwjgl.")).unwrap_or(false);

    let detail_line = if let Some(line) = line_matching_any_needle(input, exception_line, GL_ERROR_NEEDLES) {
        Some(line)
    } else if lwjgl_in_exception {
        exception_line.map(|e| e.trim().to_string())
    } else {
        None
    };

    let detail_line = detail_line?;
    Some(RuleMatch {
        kind: "gl_error",
        headline: "A graphics/driver problem prevented rendering.".to_string(),
        suggestion: "Update your GPU drivers and try again.".to_string(),
        detail: vec![detail_line],
    })
}

// ── Rule 10: mod_crash ───────────────────────────────────────────────────────────

const MOD_CRASH_HEADLINE_TEMPLATE: &str = "Crash implicates {}.";
const MOD_CRASH_SUGGESTION_TEMPLATE: &str = "Try updating or disabling {} and relaunch.";

fn rule_mod_crash(input: &AnalyzeInput, _exception_line: Option<&str>) -> Option<RuleMatch> {
    let report = input.report?;
    // Excluded loader/vanilla ids never headline — keeps rule 10 consistent with
    // resolve_suspects, whose chips filter the same ids. Jars pass through: unmatched
    // loader jars do surface as suspects there too.
    let suspect = report
        .suspect_mod_ids
        .iter()
        .find(|id| !is_excluded_suspect_id(id))
        .or_else(|| report.suspect_jars.first())?;
    Some(RuleMatch {
        kind: "mod_crash",
        headline: fmt1(MOD_CRASH_HEADLINE_TEMPLATE, suspect),
        suggestion: fmt1(MOD_CRASH_SUGGESTION_TEMPLATE, suspect),
        detail: Vec::new(),
    })
}

// ── Rule 11: generic ─────────────────────────────────────────────────────────────

const GENERIC_HEADLINE_WITH_CODE_TEMPLATE: &str = "Game crashed (exit {}).";
const GENERIC_HEADLINE_NO_CODE: &str = "Game crashed (exit code unknown).";
const GENERIC_SUGGESTION: &str = "Open the crash report and check the log for more detail.";

fn rule_generic(input: &AnalyzeInput, _exception_line: Option<&str>) -> Option<RuleMatch> {
    let headline = match input.exit_code {
        Some(code) => fmt1(GENERIC_HEADLINE_WITH_CODE_TEMPLATE, &code.to_string()),
        None => GENERIC_HEADLINE_NO_CODE.to_string(),
    };
    Some(RuleMatch { kind: "generic", headline, suggestion: GENERIC_SUGGESTION.to_string(), detail: Vec::new() })
}

// ═══════════════════════════════════════════════════════════════════════════════
// CP-3: suspect attribution (`resolve_suspects`)
// ═══════════════════════════════════════════════════════════════════════════════

/// Loader/vanilla mod ids never surfaced as crash suspects (spec §Attribution).
const EXCLUDED_SUSPECT_IDS: &[&str] = &["minecraft", "fabricloader", "forge", "neoforge"];

/// Fabric API module id prefix, also excluded (spec §Attribution: "any id starting `fabric-`").
const EXCLUDED_SUSPECT_ID_PREFIX: &str = "fabric-";

/// Maximum number of suspects surfaced (spec §Attribution: "≤3 suspects").
const MAX_SUSPECTS: usize = 3;

fn is_excluded_suspect_id(id: &str) -> bool {
    EXCLUDED_SUSPECT_IDS.contains(&id) || id.starts_with(EXCLUDED_SUSPECT_ID_PREFIX)
}

/// Cross-reference a parsed crash report's suspect ids/jars/packages against the
/// report's own mod table and the instance's mod manifest, producing a
/// priority-ordered, deduplicated, capped list of [`CrashSuspect`]s (spec
/// §Attribution). Priority: mod ids (`TRANSFORMER`/mixin annotations) > jars
/// (`~[…]`) > package fallback, ranked last.
///
/// `folder` (the reconciled mods-folder listing) is accepted per the locked CP-3
/// signature but not consulted by any current resolution rule — every rule that
/// needs a display name resolves it from `report.mods` or `mods` (`ModEntry`
/// carries `name`; `FolderMod` does not).
pub fn resolve_suspects(report: &ParsedReport, mods: &[ModEntry], _folder: &[FolderMod]) -> Vec<CrashSuspect> {
    let mut seen_ids = HashSet::new();
    let mut seen_jars = HashSet::new();
    let mut result = Vec::new();

    for id in &report.suspect_mod_ids {
        if result.len() >= MAX_SUSPECTS {
            return result;
        }
        if is_excluded_suspect_id(id) || !seen_ids.insert(id.clone()) {
            continue;
        }
        result.push(resolve_id_suspect(id, report, mods, &mut seen_jars));
    }

    for jar in &report.suspect_jars {
        if result.len() >= MAX_SUSPECTS {
            return result;
        }
        if !seen_jars.insert(normalize_jar_key(jar)) {
            continue;
        }
        result.push(resolve_jar_suspect(jar, mods));
    }

    for pkg in &report.suspect_packages {
        if result.len() >= MAX_SUSPECTS {
            return result;
        }
        result.push(CrashSuspect { display: pkg.clone(), mod_id: None, jar: None });
    }

    result
}

/// Resolve a single suspect mod id: the report's own mod table first (display
/// name + jar when present), then a best-effort instance-manifest match by
/// normalized name, then the bare id as a last resort. Registers any jar found
/// along the way into `seen_jars` so the jar pass doesn't re-add it (spec: "a
/// suspect found via id AND jar appears once").
fn resolve_id_suspect(
    id: &str,
    report: &ParsedReport,
    mods: &[ModEntry],
    seen_jars: &mut HashSet<String>,
) -> CrashSuspect {
    if let Some(rm) = report.mods.iter().find(|m| m.id == id) {
        if let Some(jar) = &rm.jar {
            seen_jars.insert(normalize_jar_key(jar));
        }
        return CrashSuspect { display: rm.name.clone(), mod_id: Some(id.to_string()), jar: rm.jar.clone() };
    }

    if let Some(entry) = find_manifest_match_by_id(id, mods) {
        seen_jars.insert(normalize_jar_key(&entry.file_name));
        return CrashSuspect {
            display: entry.name.clone().unwrap_or_else(|| id.to_string()),
            mod_id: Some(id.to_string()),
            jar: Some(entry.file_name.clone()),
        };
    }

    CrashSuspect { display: id.to_string(), mod_id: Some(id.to_string()), jar: None }
}

/// Best-effort fallback for the "else → instance manifest match" tier (spec
/// §Attribution): compares the mod id against each `ModEntry`'s captured
/// display `name`, both normalized (lowercased, non-alphanumeric stripped) —
/// covers the common case where a mod's loader id and its display name agree
/// once punctuation/casing/spacing is ignored (e.g. id `examplemod` / name
/// `"Example Mod"`).
fn find_manifest_match_by_id<'a>(id: &str, mods: &'a [ModEntry]) -> Option<&'a ModEntry> {
    let norm_id = normalize_ident(id);
    mods.iter().find(|m| m.name.as_deref().map(normalize_ident).as_deref() == Some(norm_id.as_str()))
}

fn normalize_ident(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// Resolve a single suspect jar: match against `ModEntry::file_name`
/// case-insensitively (tolerating a `.disabled` suffix on either side) for the
/// display name; an unmatched jar surfaces with the jar itself as the display.
fn resolve_jar_suspect(jar: &str, mods: &[ModEntry]) -> CrashSuspect {
    let jar_key = normalize_jar_key(jar);
    if let Some(entry) = mods.iter().find(|m| normalize_jar_key(&m.file_name) == jar_key) {
        return CrashSuspect {
            display: entry.name.clone().unwrap_or_else(|| jar.to_string()),
            mod_id: None,
            jar: Some(jar.to_string()),
        };
    }
    CrashSuspect { display: jar.to_string(), mod_id: None, jar: Some(jar.to_string()) }
}

/// Normalize a jar filename for matching/dedup purposes: lowercased, with a
/// trailing `.disabled` suffix stripped (spec: "tolerating `.jar` vs `.jar.disabled`").
fn normalize_jar_key(jar: &str) -> String {
    let lower = jar.to_lowercase();
    lower.strip_suffix(".disabled").map(str::to_string).unwrap_or(lower)
}

#[cfg(test)]
#[path = "crash_tests.rs"]
mod tests;
