//! Pure crash-report parser (CP-1 of the crash-log-help spec).
//!
//! `parse_crash_report` turns the raw text of a Minecraft `mc/crash-reports/*.txt` file
//! into a [`ParsedReport`]. Everything here is a pure `&str -> struct` transform: no
//! Tauri types, no `regex` crate, no network, no I/O. Malformed, truncated, or empty
//! input never panics — fields simply come back `None`/empty.
//!
//! Downstream checkpoints build on this shape:
//! - CP-2 (`analyze`) turns a [`ParsedReport`] + log tail into a [`CrashAnalysis`]
//!   (not implemented yet — do not stub it here).
//! - CP-3 (`resolve_suspects`) cross-references [`ParsedReport::mods`] and the
//!   suspect vectors against the instance's mod manifest (not implemented yet).

use std::collections::HashSet;

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

#[cfg(test)]
#[path = "crash_tests.rs"]
mod tests;
