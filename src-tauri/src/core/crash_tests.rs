//! Unit tests for `crash` (CP-1: crash-report parser; CP-2: rule engine). Wired via
//! `#[cfg(test)] #[path = "crash_tests.rs"] mod tests;`. All tests are pure — no I/O.

use super::*;

const REPORT_FABRIC: &str = include_str!("fixtures/crash/report_fabric_classcast.txt");
const REPORT_NEOFORGE: &str = include_str!("fixtures/crash/report_neoforge_annotated.txt");
const REPORT_VANILLA: &str = include_str!("fixtures/crash/report_vanilla_simple.txt");
const REPORT_OOM: &str = include_str!("fixtures/crash/report_oom.txt");

const LOG_FABRIC_UNMET_DEPS: &str = include_str!("fixtures/crash/log_fabric_unmet_deps.txt");
const LOG_FORGE_MISSING_DEPS: &str = include_str!("fixtures/crash/log_forge_missing_deps.txt");
const LOG_MIXIN_FAIL: &str = include_str!("fixtures/crash/log_mixin_fail.txt");
const LOG_DUPLICATE_MODS: &str = include_str!("fixtures/crash/log_duplicate_mods.txt");
const LOG_GLFW: &str = include_str!("fixtures/crash/log_glfw.txt");

// ── Description / Time ─────────────────────────────────────────────────────────

#[test]
fn cp1_description_and_time_extracted_fabric() {
    let report = parse_crash_report(REPORT_FABRIC);
    assert_eq!(report.description.as_deref(), Some("Rendering overlay"));
    assert_eq!(report.time.as_deref(), Some("7/23/26, 3:14 PM"));
}

#[test]
fn cp1_description_and_time_extracted_neoforge() {
    let report = parse_crash_report(REPORT_NEOFORGE);
    assert_eq!(report.description.as_deref(), Some("Ticking entity"));
    assert_eq!(report.time.as_deref(), Some("7/23/26, 4:02 PM"));
}

// ── Exception: class + message + frames ────────────────────────────────────────

#[test]
fn cp1_exception_class_and_message_fabric() {
    let report = parse_crash_report(REPORT_FABRIC);
    let exc = report.exception.expect("exception must be parsed");
    assert_eq!(exc.class, "java.lang.ClassCastException");
    assert_eq!(
        exc.message.as_deref(),
        Some("class net.minecraft.class_1122 cannot be cast to class net.minecraft.class_3311")
    );
    assert_eq!(exc.frames.len(), 4);
    assert_eq!(exc.frames[0], "net.minecraft.class_310.method_1587(class_310.java:1234)");
}

#[test]
fn cp1_exception_no_message_oom() {
    let report = parse_crash_report(REPORT_OOM);
    let exc = report.exception.expect("exception must be parsed");
    assert_eq!(exc.class, "java.lang.OutOfMemoryError");
    assert_eq!(exc.message.as_deref(), Some("Java heap space"));
}

#[test]
fn cp1_exception_frames_capped_at_15() {
    let mut text = String::from("Description: too many frames\n\ncom.example.BigException: boom\n");
    for i in 0..20 {
        text.push_str(&format!("\tat com.example.Frame{i}.run(Frame{i}.java:{i})\n"));
    }
    let report = parse_crash_report(&text);
    let exc = report.exception.expect("exception must be parsed");
    assert_eq!(exc.frames.len(), 15, "frames must be capped at 15 even though 20 were present");
    assert_eq!(exc.frames[0], "com.example.Frame0.run(Frame0.java:0)");
    assert_eq!(exc.frames[14], "com.example.Frame14.run(Frame14.java:14)");
}

// ── Minecraft Version ───────────────────────────────────────────────────────────

#[test]
fn cp1_minecraft_version_extracted_not_confused_with_version_id() {
    let report = parse_crash_report(REPORT_FABRIC);
    assert_eq!(report.minecraft_version.as_deref(), Some("1.20.1"));
}

#[test]
fn cp1_minecraft_version_extracted_neoforge() {
    let report = parse_crash_report(REPORT_NEOFORGE);
    assert_eq!(report.minecraft_version.as_deref(), Some("1.21"));
}

// ── Fabric Mods: table ──────────────────────────────────────────────────────────

#[test]
fn cp1_fabric_mods_table_parsed() {
    let report = parse_crash_report(REPORT_FABRIC);
    assert_eq!(report.mods.len(), 4);

    let example = report.mods.iter().find(|m| m.id == "examplemod").expect("examplemod present");
    assert_eq!(example.name, "Example Mod");
    assert_eq!(example.version, "1.0.0");
    assert_eq!(example.jar, None);

    let loader = report.mods.iter().find(|m| m.id == "fabricloader").expect("fabricloader present");
    assert_eq!(loader.name, "Fabric Loader");
    assert_eq!(loader.version, "0.15.7");
}

// ── Mod List: pipe table ─────────────────────────────────────────────────────────

#[test]
fn cp1_mod_list_pipe_table_parsed() {
    let report = parse_crash_report(REPORT_NEOFORGE);
    assert_eq!(report.mods.len(), 3);

    let example = report.mods.iter().find(|m| m.id == "examplemod").expect("examplemod present");
    assert_eq!(example.name, "Example Mod");
    assert_eq!(example.version, "1.0.0");
    assert_eq!(example.jar.as_deref(), Some("examplemod-1.0.0.jar"));

    let neoforge = report.mods.iter().find(|m| m.id == "neoforge").expect("neoforge present");
    assert_eq!(neoforge.jar.as_deref(), Some("neoforge-21.1.209-universal.jar"));
    assert_eq!(neoforge.version, "21.1.209");
}

// ── Suspect mod ids: TRANSFORMER/<id>@ and {mixin from mod <id>} ────────────────

#[test]
fn cp1_suspect_mod_ids_from_transformer_and_mixin_annotations() {
    let report = parse_crash_report(REPORT_NEOFORGE);
    assert!(report.suspect_mod_ids.contains(&"examplemod".to_string()));
    assert!(report.suspect_mod_ids.contains(&"neoforge".to_string()));
    // examplemod appears via both TRANSFORMER/ and {mixin from mod } on the same frame —
    // must be deduplicated, not doubled.
    let examplemod_count =
        report.suspect_mod_ids.iter().filter(|id| id.as_str() == "examplemod").count();
    assert_eq!(examplemod_count, 1);
}

#[test]
fn cp1_suspect_mod_ids_none_when_no_annotations() {
    let report = parse_crash_report(REPORT_FABRIC);
    assert!(report.suspect_mod_ids.is_empty());
}

// ── Suspect jars: ~[<jar>...] with vanilla filtering ────────────────────────────

#[test]
fn cp1_suspect_jars_extracted_and_vanilla_filtered() {
    let report = parse_crash_report(REPORT_NEOFORGE);
    assert!(report.suspect_jars.contains(&"examplemod-1.0.0.jar".to_string()));
    assert!(report.suspect_jars.contains(&"neoforge-21.1.209-universal.jar".to_string()));
    // client-1.21.jar and server-1.21.jar are vanilla-ish (client-*/server-* prefix) — filtered.
    assert!(!report.suspect_jars.iter().any(|j| j.starts_with("client-")));
    assert!(!report.suspect_jars.iter().any(|j| j.starts_with("server-")));
}

#[test]
fn cp1_suspect_jars_bare_question_mark_filtered() {
    let text = "Description: d\n\ncom.example.Foo: msg\n\tat com.example.Foo.bar(Foo.java:1) ~[?:?] {}\n";
    let report = parse_crash_report(text);
    assert!(report.suspect_jars.is_empty(), "bare '?' bootstrap frames must not produce a suspect jar");
}

#[test]
fn cp1_suspect_jars_deduplicated() {
    let text = concat!(
        "Description: d\n\n",
        "com.example.Foo: msg\n",
        "\tat com.example.Foo.bar(Foo.java:1) ~[modjar-1.0.jar:?] {}\n",
        "\tat com.example.Foo.baz(Foo.java:2) ~[modjar-1.0.jar:?] {}\n",
    );
    let report = parse_crash_report(text);
    assert_eq!(report.suspect_jars, vec!["modjar-1.0.jar".to_string()]);
}

// ── Package-prefix fallback ──────────────────────────────────────────────────────

#[test]
fn cp1_suspect_packages_fallback_skips_exclusion_list() {
    let text = concat!(
        "Description: d\n\n",
        "com.example.Foo: msg\n",
        "\tat net.minecraft.class_310.method_1(class_310.java:1)\n", // excluded (net.minecraft.)
        "\tat com.example.modone.SomeClass.doThing(SomeClass.java:2)\n",
        "\tat org.lwjgl.opengl.GL11.glClear(GL11.java:3)\n", // excluded (org.lwjgl.)
        "\tat com.example.modtwo.OtherClass.doOther(OtherClass.java:4)\n",
        "\tat com.example.modthree.ThirdClass.doThird(ThirdClass.java:5)\n",
    );
    let report = parse_crash_report(text);
    assert_eq!(
        report.suspect_packages,
        vec!["com.example.modone".to_string(), "com.example.modtwo".to_string()],
        "must take the first 2 non-excluded packages, skipping net.minecraft./org.lwjgl."
    );
}

#[test]
fn cp1_suspect_packages_all_excluded_yields_empty() {
    let report = parse_crash_report(REPORT_FABRIC);
    // Every frame in the Fabric fixture is net.minecraft.* — no fallback candidates.
    assert!(report.suspect_packages.is_empty());
}

#[test]
fn cp1_suspect_packages_transformer_prefix_stripped() {
    let text = concat!(
        "Description: d\n\n",
        "com.example.Foo: msg\n",
        "\tat TRANSFORMER/examplemod@1.0/com.example.examplemod.Hook.run(Hook.java:1) ~[examplemod.jar:?] {}\n",
    );
    let report = parse_crash_report(text);
    // suspect_mod_ids should already cover this case (non-empty), but the package
    // extractor itself must still strip the TRANSFORMER/ prefix correctly.
    assert!(report.suspect_packages.contains(&"com.example.examplemod".to_string()));
}

// ── Vanilla simple fixture (no loader sections) ─────────────────────────────────

#[test]
fn cp1_vanilla_report_no_mods_no_suspects() {
    let report = parse_crash_report(REPORT_VANILLA);
    assert!(report.mods.is_empty());
    assert!(report.suspect_mod_ids.is_empty());
    assert!(report.suspect_jars.is_empty());
    let exc = report.exception.expect("exception must be parsed");
    assert_eq!(exc.class, "java.lang.NullPointerException");
    assert_eq!(report.minecraft_version.as_deref(), Some("1.20.4"));
}

// ── Malformed / truncated / empty input ─────────────────────────────────────────

#[test]
fn cp1_empty_input_no_panic_all_none_empty() {
    let report = parse_crash_report("");
    assert_eq!(report.description, None);
    assert_eq!(report.time, None);
    assert_eq!(report.exception, None);
    assert_eq!(report.minecraft_version, None);
    assert!(report.suspect_mod_ids.is_empty());
    assert!(report.suspect_jars.is_empty());
    assert!(report.suspect_packages.is_empty());
    assert!(report.mods.is_empty());
}

#[test]
fn cp1_garbage_input_no_panic() {
    let report = parse_crash_report("this is not a crash report at all\njust some\nrandom lines\n{}[]~%!@#$");
    assert_eq!(report.exception, None);
    assert_eq!(report.minecraft_version, None);
    assert!(report.mods.is_empty());
}

#[test]
fn cp1_truncated_exception_header_no_panic() {
    // Header ends mid-message with no frames, no System Details, nothing after.
    let report = parse_crash_report("Description: cut off\n\ncom.example.Truncated: something went");
    let exc = report.exception.expect("class-only exception should still parse");
    assert_eq!(exc.class, "com.example.Truncated");
    assert_eq!(exc.message.as_deref(), Some("something went"));
    assert!(exc.frames.is_empty());
}

#[test]
fn cp1_truncated_tilde_bracket_no_panic() {
    // A frame with an unterminated `~[` annotation must not panic on slicing.
    let text = "Description: d\n\ncom.example.Foo: msg\n\tat com.example.Foo.bar(Foo.java:1) ~[unterminated";
    let report = parse_crash_report(text);
    // No '%'/'!'/':'/']' found — the whole trailing text becomes the "jar", which is
    // harmless (not a vanilla prefix, not empty) — the key assertion is just: no panic.
    assert!(!report.suspect_jars.is_empty());
}

#[test]
fn cp1_no_exception_header_returns_none() {
    let report = parse_crash_report("Description: d\nTime: t\n\n-- System Details --\nDetails:\n\tMinecraft Version: 1.20.1\n");
    assert_eq!(report.exception, None);
    assert_eq!(report.minecraft_version.as_deref(), Some("1.20.1"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// CP-2: rule engine (`analyze`)
// ═══════════════════════════════════════════════════════════════════════════════

fn lines_of(s: &str) -> Vec<String> {
    s.lines().map(|l| l.to_string()).collect()
}

fn blank_input<'a>() -> AnalyzeInput<'a> {
    AnalyzeInput {
        report: None,
        report_text: None,
        log_tail: &[],
        exit_code: Some(1),
        report_path: None,
        jvm_error_path: None,
    }
}

// ── Rule 1: fabric_unmet_deps ───────────────────────────────────────────────────

#[test]
fn cp2_fabric_unmet_deps_matches_in_exception() {
    let text = "Description: d\n\nnet.fabricmc.loader.impl.FormattedException: Mod resolution encountered an incompatible mod set!\n";
    let report = parse_crash_report(text);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "fabric_unmet_deps");
}

#[test]
fn cp2_fabric_unmet_deps_matches_in_log_tail_with_detail_lines() {
    let log_tail = lines_of(LOG_FABRIC_UNMET_DEPS);
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "fabric_unmet_deps");
    assert!(analysis.detail.iter().any(|l| l.contains("othermod")));
    assert!(analysis.detail.iter().any(|l| l.contains("thirdmod")));
}

#[test]
fn cp2_unmet_dep_detail_lines_capped_at_12() {
    let mut log = String::from("Unmet dependency listing:\n");
    for i in 0..20 {
        log.push_str(&format!(
            "- Mod 'Mod{i}' (mod{i}) 1.0 requires version 2.0 or later of mod 'Dep{i}' (dep{i}), which is missing!\n"
        ));
    }
    let log_tail = lines_of(&log);
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "fabric_unmet_deps");
    assert_eq!(analysis.detail.len(), 12, "must cap verbatim unmet-dep lines at 12 even with 20 available");
}

// ── Rule 2: forge_missing_deps ──────────────────────────────────────────────────

#[test]
fn cp2_forge_missing_deps_matches_in_exception() {
    let text = "Description: d\n\ncpw.mods.modlauncher.InvalidLauncherSetupException: Missing or unsupported mandatory dependencies to launch\n";
    let report = parse_crash_report(text);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "forge_missing_deps");
}

#[test]
fn cp2_forge_missing_deps_matches_in_log_tail_with_detail_block() {
    let log_tail = lines_of(LOG_FORGE_MISSING_DEPS);
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "forge_missing_deps");
    assert_eq!(analysis.detail.len(), 2);
    assert!(analysis.detail[0].contains("somemod"));
}

// ── Rule 3: duplicate_mods ──────────────────────────────────────────────────────

#[test]
fn cp2_duplicate_mods_matches_in_exception() {
    let text = "Description: d\n\nnet.minecraftforge.fml.loading.moddiscovery.DuplicateModsFoundException: Found duplicate mods: examplemod\n";
    let report = parse_crash_report(text);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "duplicate_mods");
}

#[test]
fn cp2_duplicate_mods_matches_in_log_tail_with_detail() {
    let log_tail = lines_of(LOG_DUPLICATE_MODS);
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "duplicate_mods");
    assert!(analysis.detail.iter().any(|l| l.contains("examplemod")));
}

// ── Rule 4: out_of_memory ───────────────────────────────────────────────────────

#[test]
fn cp2_out_of_memory_matches_via_exception_and_beats_generic() {
    let report = parse_crash_report(REPORT_OOM);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(REPORT_OOM);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "out_of_memory");
    assert!(analysis.detail.iter().any(|l| l.contains("Java heap space")));
}

#[test]
fn cp2_out_of_memory_matches_in_log_tail() {
    let log_tail =
        vec!["Exception in thread \"main\" java.lang.OutOfMemoryError: GC overhead limit exceeded".to_string()];
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "out_of_memory");
    assert!(analysis.detail[0].contains("GC overhead limit exceeded"));
}

// ── Rule 5: unsupported_java ────────────────────────────────────────────────────

#[test]
fn cp2_unsupported_java_matches_via_exception() {
    let text = concat!(
        "Description: d\n\n",
        "java.lang.UnsupportedClassVersionError: com/example/Foo has been compiled by a more recent ",
        "version of the Java Runtime (class file version 65.0)\n",
    );
    let report = parse_crash_report(text);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "unsupported_java");
    assert!(analysis.detail[0].contains("compiled by a more recent version"));
}

#[test]
fn cp2_unsupported_java_matches_in_log_tail() {
    let log_tail = vec![
        "Caused by: java.lang.UnsupportedClassVersionError: com/example/Foo has unsupported class file"
            .to_string(),
    ];
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "unsupported_java");
}

// ── Rule 6: mixin_failure ───────────────────────────────────────────────────────

#[test]
fn cp2_mixin_failure_matches_via_exception_frames_only() {
    let report = parse_crash_report(REPORT_NEOFORGE);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(REPORT_NEOFORGE);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "mixin_failure");
    assert!(analysis.detail[0].contains("mixin from mod examplemod"));
}

#[test]
fn cp2_mixin_failure_matches_in_log_tail_via_named_needle() {
    let log_tail = lines_of(LOG_MIXIN_FAIL);
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "mixin_failure");
}

// ── Rule 7: missing_class ───────────────────────────────────────────────────────

#[test]
fn cp2_missing_class_matches_via_exception_classnotfound() {
    let text = "Description: d\n\njava.lang.ClassNotFoundException: com.example.MissingClass\n";
    let report = parse_crash_report(text);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "missing_class");
    assert_eq!(analysis.detail, vec!["com.example.MissingClass".to_string()]);
}

#[test]
fn cp2_missing_class_matches_in_log_tail_noclassdef() {
    let log_tail = vec!["Caused by: java.lang.NoClassDefFoundError: com/example/OtherClass".to_string()];
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "missing_class");
    assert!(analysis.detail[0].contains("OtherClass"));
}

// ── Rule 8: native_crash ────────────────────────────────────────────────────────

#[test]
fn cp2_native_crash_fires_on_windows_access_violation_exit_code() {
    let mut input = blank_input();
    input.exit_code = Some(-1073741819);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "native_crash");
}

#[test]
fn cp2_native_crash_fires_on_jvm_error_path_with_unknown_exit_code() {
    let mut input = blank_input();
    input.exit_code = None;
    input.jvm_error_path = Some("hs_err_pid123.log".to_string());
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "native_crash");
    assert_eq!(analysis.jvm_error_path.as_deref(), Some("hs_err_pid123.log"));
}

// ── Rule 9: gl_error ─────────────────────────────────────────────────────────────

#[test]
fn cp2_gl_error_matches_in_log_tail() {
    let log_tail = lines_of(LOG_GLFW);
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "gl_error");
    assert!(analysis.detail[0].contains("GLFW error"));
}

#[test]
fn cp2_gl_error_lwjgl_needle_matches_only_in_exception() {
    let text = "Description: d\n\norg.lwjgl.LWJGLException: failed to create display\n";
    let report = parse_crash_report(text);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "gl_error");
}

#[test]
fn cp2_gl_error_lwjgl_needle_in_log_tail_does_not_match() {
    let log_tail = vec!["at org.lwjgl.opengl.GL11.glClear(GL11.java:3)".to_string()];
    let mut input = blank_input();
    input.log_tail = &log_tail;
    let analysis = analyze(input);
    assert_eq!(
        analysis.kind, "generic",
        "org.lwjgl. needle must only apply to the exception line, not the log tail"
    );
}

// ── Rule 10: mod_crash ──────────────────────────────────────────────────────────

#[test]
fn cp2_mod_crash_fallback_when_report_has_suspect_mod_ids() {
    let text = concat!(
        "Description: d\n\n",
        "com.example.Foo: boom\n",
        "\tat TRANSFORMER/examplemod@1.0/com.example.examplemod.Hook.run(Hook.java:1) ~[examplemod.jar:?] {}\n",
    );
    let report = parse_crash_report(text);
    assert!(report.suspect_mod_ids.contains(&"examplemod".to_string()));
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "mod_crash");
    assert!(analysis.headline.contains("examplemod"));
}

#[test]
fn cp2_mod_crash_fallback_when_report_has_only_suspect_jars() {
    let text = concat!(
        "Description: d\n\n",
        "com.example.Foo: boom\n",
        "\tat com.example.somelib.Helper.run(Helper.java:1) ~[somelib-1.2.jar:?] {}\n",
    );
    let report = parse_crash_report(text);
    assert!(report.suspect_mod_ids.is_empty());
    assert!(report.suspect_jars.contains(&"somelib-1.2.jar".to_string()));
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "mod_crash");
    assert!(analysis.headline.contains("somelib-1.2.jar"));
}

// ── Rule 11: generic ─────────────────────────────────────────────────────────────

#[test]
fn cp2_generic_fallback_mentions_exit_code_when_known() {
    let mut input = blank_input();
    input.exit_code = Some(1);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "generic");
    assert!(analysis.headline.contains('1'));
    assert!(!analysis.suggestion.is_empty());
}

#[test]
fn cp2_generic_fallback_non_empty_when_exit_code_unknown() {
    let mut input = blank_input();
    input.exit_code = None;
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "generic");
    assert!(!analysis.headline.is_empty());
    assert!(!analysis.suggestion.is_empty());
}

// ── Priority ordering ────────────────────────────────────────────────────────────

#[test]
fn cp2_priority_fabric_unmet_deps_beats_missing_class() {
    let text = concat!(
        "Description: d\n\n",
        "java.lang.ClassNotFoundException: com.example.Missing\n",
        "Unmet dependency listing:\n",
        "- Mod 'A' (a) 1.0 requires version 2.0 or later of mod 'B' (b), which is missing!\n",
    );
    let report = parse_crash_report(text);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(text);
    let analysis = analyze(input);
    assert_eq!(analysis.kind, "fabric_unmet_deps", "rule 1 must win over rule 7 when both needles are present");
}

// ── report_path / jvm_error_path carried through ─────────────────────────────────

#[test]
fn cp2_report_path_and_jvm_error_path_carried_through() {
    let mut input = blank_input();
    input.report_path = Some("crash-reports/x.txt".to_string());
    input.jvm_error_path = Some("hs_err_pid1.log".to_string());
    let analysis = analyze(input);
    assert_eq!(analysis.report_path.as_deref(), Some("crash-reports/x.txt"));
    assert_eq!(analysis.jvm_error_path.as_deref(), Some("hs_err_pid1.log"));
}

// ── CrashAnalysis.exception single-line shape ────────────────────────────────────

#[test]
fn cp2_exception_field_is_class_colon_message_single_line() {
    let report = parse_crash_report(REPORT_OOM);
    let mut input = blank_input();
    input.report = Some(&report);
    input.report_text = Some(REPORT_OOM);
    let analysis = analyze(input);
    assert_eq!(analysis.exception.as_deref(), Some("java.lang.OutOfMemoryError: Java heap space"));
}

#[test]
fn cp2_exception_field_none_when_no_report() {
    let input = blank_input();
    let analysis = analyze(input);
    assert_eq!(analysis.exception, None);
}
