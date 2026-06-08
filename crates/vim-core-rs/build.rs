use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod build_artifact {
    include!("build_artifact.rs");
}
mod build_allowlist {
    include!("build_allowlist.rs");
}
mod build_compile_plan {
    include!("build_compile_plan.rs");
}
mod build_link_audit {
    include!("build_link_audit.rs");
}
mod build_test_runner {
    include!("build_test_runner.rs");
}

use build_allowlist::{Allowlist, validate_allowlist, verify_bridge_header};
use build_artifact::{
    emit_artifact_rerun_if_env_changed, install_prebuilt_artifact,
    resolve_artifact_config_from_env, source_build_requested, verbose_build_requested,
};
use build_compile_plan::{CompilePlan, UpstreamMetadata, create_compile_plan, write_compile_proof};

struct GeneratedVimBuildArtifacts {
    include_root: PathBuf,
    generated_sources: Vec<PathBuf>,
    staged_vendor_src_root: PathBuf,
}

fn main() {
    if let Err(error) = run() {
        panic!("{error}");
    }
}

fn run() -> Result<(), String> {
    emit_artifact_rerun_if_env_changed();

    let repo_root = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|error| format!("missing CARGO_MANIFEST_DIR: {error}"))?,
    );
    let out_dir =
        PathBuf::from(env::var("OUT_DIR").map_err(|error| format!("missing OUT_DIR: {error}"))?);
    let native_dir = repo_root.join("native");
    let vendor_dir = repo_root.join("vendor/vim_src");
    let bridge_header = native_dir.join("vim_bridge.h");
    let allowlist_path = repo_root.join("vim-source-allowlist.txt");
    let manifest_path = repo_root.join("vim-source-build-manifest.txt");
    let metadata_path = repo_root.join("upstream-metadata.json");
    let skiplist_path = repo_root.join("upstream-test-skiplist.txt");
    let classification_manifest_path = repo_root.join("upstream-test-classification.json");
    let upstream_testdir_path = repo_root.join("vendor/vim_src/src/testdir");
    emit_static_rerun_if_changed(&[&bridge_header, &native_dir]);

    if !source_build_requested() {
        emit_static_rerun_if_changed(&[
            &skiplist_path,
            &classification_manifest_path,
            &upstream_testdir_path,
            &repo_root.join("vendor/upstream/vim/runtime"),
        ]);
        let artifact_config = resolve_artifact_config_from_env()?;
        let prepared = install_prebuilt_artifact(&artifact_config, &out_dir)?;
        refresh_prebuilt_runtime_contract(
            &repo_root,
            &out_dir,
            &prepared.manifest.upstream_vim_tag,
        )?;
        generate_bindings(&bridge_header, &out_dir)?;
        compile_native_overlay_archive(&repo_root, &out_dir)?;
        compile_prebuilt_runtime_contract_archive(&repo_root, &out_dir)?;
        build_test_runner::generate_upstream_tests(&out_dir)?;
        if verbose_build_requested() {
            println!(
                "cargo:warning=using prebuilt vim-core-rs artifact target={} cache_key={}",
                prepared.manifest.target_triple, prepared.cache_key
            );
        }
        println!("cargo:rustc-link-search=native={}", out_dir.display());
        println!("cargo:rustc-link-lib=static=vimcore_prebuilt_runtime_contract");
        println!("cargo:rustc-link-lib=static=vimcore_native_overlay");
        println!("cargo:rustc-link-lib=static=vimcore");
        if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
            println!("cargo:rustc-link-lib=iconv");
        }
        return Ok(());
    }

    emit_static_rerun_if_changed(&[
        &bridge_header,
        &allowlist_path,
        &manifest_path,
        &metadata_path,
        &skiplist_path,
        &classification_manifest_path,
        &native_dir,
        &vendor_dir,
    ]);

    let metadata = UpstreamMetadata::load(&metadata_path)?;
    verify_bridge_header(&bridge_header)?;

    let allowlist = Allowlist::load(&allowlist_path)?;
    validate_allowlist(&repo_root, &vendor_dir, &allowlist)?;

    let compile_plan = create_compile_plan(&repo_root, &native_dir, &manifest_path, &allowlist)?;
    emit_plan_rerun_if_changed(&compile_plan);
    let generated_vim_build =
        prepare_generated_vim_build(&repo_root, &vendor_dir.join("src"), &out_dir, &metadata)?;

    let native_archive =
        compile_native_archive(&repo_root, &out_dir, &compile_plan, &generated_vim_build)?;
    let vendor_archive =
        compile_vendor_archive(&repo_root, &out_dir, &compile_plan, &generated_vim_build)?;
    let final_archive = combine_archives(&out_dir, &[native_archive, vendor_archive])?;

    build_link_audit::run_link_audit(
        &final_archive,
        &out_dir,
        &compile_plan,
        &generated_vim_build.generated_sources,
    )?;
    generate_bindings(&bridge_header, &out_dir)?;
    build_test_runner::generate_upstream_tests(&out_dir)?;
    write_compile_proof(&out_dir, &metadata, &compile_plan)?;

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=vimcore");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=iconv");
    }

    Ok(())
}

fn emit_static_rerun_if_changed(paths: &[&Path]) {
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn compile_native_overlay_archive(repo_root: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let generated_include_root = out_dir.join("vim_build");
    let files = build_compile_plan::collect_native_sources(repo_root, &repo_root.join("native"))?;
    compile_archive(
        repo_root,
        out_dir,
        "vimcore_native_overlay",
        &files,
        &generated_include_root,
        WarningPolicy::Strict,
        &[],
    )
}

fn compile_prebuilt_runtime_contract_archive(
    repo_root: &Path,
    out_dir: &Path,
) -> Result<PathBuf, String> {
    let generated_include_root = out_dir.join("vim_build");
    compile_archive(
        repo_root,
        out_dir,
        "vimcore_prebuilt_runtime_contract",
        &[generated_include_root.join("auto").join("pathdef.c")],
        &generated_include_root,
        WarningPolicy::Strict,
        &[],
    )
}

fn emit_plan_rerun_if_changed(plan: &CompilePlan) {
    for path in plan.native_sources.iter().chain(plan.vendor_sources.iter()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn compile_native_archive(
    repo_root: &Path,
    out_dir: &Path,
    compile_plan: &CompilePlan,
    generated_vim_build: &GeneratedVimBuildArtifacts,
) -> Result<PathBuf, String> {
    let files = compile_plan
        .native_sources
        .iter()
        .map(|path| repo_root.join(path))
        .collect::<Vec<_>>();

    compile_archive(
        repo_root,
        out_dir,
        "vimcore_native",
        &files,
        &generated_vim_build.include_root,
        WarningPolicy::Strict,
        &[],
    )
}

fn compile_vendor_archive(
    repo_root: &Path,
    out_dir: &Path,
    compile_plan: &CompilePlan,
    generated_vim_build: &GeneratedVimBuildArtifacts,
) -> Result<PathBuf, String> {
    let vendor_src_root = repo_root.join("vendor/vim_src/src");
    let vendor_files = compile_plan
        .vendor_sources
        .iter()
        .map(|path| repo_root.join(path))
        .map(|path| {
            path.strip_prefix(&vendor_src_root)
                .ok()
                .filter(|relative| relative.starts_with("xdiff"))
                .map(|relative| generated_vim_build.staged_vendor_src_root.join(relative))
                .unwrap_or(path)
        })
        .collect::<Vec<_>>();
    let mut regular_files = Vec::new();
    let mut archives = Vec::new();

    for file in vendor_files {
        if file.ends_with("vendor/vim_src/src/main.c") {
            archives.push(compile_archive(
                repo_root,
                out_dir,
                "vimcore_vendor_main",
                &[file],
                &generated_vim_build.include_root,
                WarningPolicy::Relaxed,
                &[
                    ("main", "vim_core_embedded_main"),
                    ("getout", "vim_core_embedded_getout"),
                ],
            )?);
        } else if file.ends_with("vendor/vim_src/src/os_unix.c") {
            archives.push(compile_archive(
                repo_root,
                out_dir,
                "vimcore_vendor_os_unix",
                &[file],
                &generated_vim_build.include_root,
                WarningPolicy::Relaxed,
                &[
                    ("mch_inchar", "vim_core_vendor_mch_inchar"),
                    ("mch_job_start", "vim_core_vendor_mch_job_start"),
                    ("mch_job_status", "vim_core_vendor_mch_job_status"),
                    (
                        "mch_detect_ended_job",
                        "vim_core_vendor_mch_detect_ended_job",
                    ),
                    ("mch_signal_job", "vim_core_vendor_mch_signal_job"),
                    ("mch_clear_job", "vim_core_vendor_mch_clear_job"),
                ],
            )?);
        } else {
            regular_files.push(file);
        }
    }
    regular_files.extend(generated_vim_build.generated_sources.iter().cloned());

    archives.insert(
        0,
        compile_archive(
            repo_root,
            out_dir,
            "vimcore_vendor",
            &regular_files,
            &generated_vim_build.include_root,
            WarningPolicy::Relaxed,
            &[],
        )?,
    );

    combine_archives_named(out_dir, "libvimcore_vendor_combined.a", &archives)
}

enum WarningPolicy {
    Strict,
    Relaxed,
}

fn compile_archive(
    repo_root: &Path,
    out_dir: &Path,
    library_name: &str,
    files: &[PathBuf],
    generated_include_root: &Path,
    warning_policy: WarningPolicy,
    extra_defines: &[(&str, &str)],
) -> Result<PathBuf, String> {
    if files.is_empty() {
        return Err(format!("{library_name} does not have source files"));
    }

    let mut build = cc::Build::new();
    build.out_dir(out_dir);
    build.cargo_metadata(false);
    build.include(repo_root.join("native"));
    build.include(generated_include_root);
    build.include(repo_root.join("vendor/vim_src/src"));
    build.include(repo_root.join("vendor/vim_src/src/proto"));
    build.define("HAVE_CONFIG_H", None);
    build.define("FEAT_JOB_CHANNEL", "1");
    for (key, value) in extra_defines {
        build.define(key, Some(*value));
    }

    // Vendor Vim is split into multiple archives (for example os_unix/main).
    // Keep all of them behind the VFD hooks so embedded mode never bypasses
    // host-owned stdio/job fd handling through a split archive.
    if library_name.starts_with("vimcore_vendor") {
        build.define("read", "vim_bridge_vfd_read");
        build.define("write", "vim_bridge_vfd_write");
        build.define("close", "vim_bridge_vfd_close");
        build.define("poll", "vim_bridge_vfd_poll");
        build.define("select", "vim_bridge_vfd_select");
    }

    match warning_policy {
        WarningPolicy::Strict => {
            build.warnings(true);
            build.warnings_into_errors(true);
        }
        WarningPolicy::Relaxed => {
            build.warnings(false);
        }
    }

    for file in files {
        build.file(file);
    }

    build.compile(library_name);

    Ok(out_dir.join(format!("lib{library_name}.a")))
}

fn prepare_generated_vim_build(
    repo_root: &Path,
    vendor_src_dir: &Path,
    out_dir: &Path,
    metadata: &UpstreamMetadata,
) -> Result<GeneratedVimBuildArtifacts, String> {
    const VIM_MODIFIED_BY: &str =
        "vim-core-rs maintainers https://github.com/shun/vim-core-rs/issues";

    let build_root = out_dir.join("vim_build");
    let auto_dir = build_root.join("auto");
    let staged_vendor_src_root = build_root.join("src");
    fs::create_dir_all(&auto_dir).map_err(|error| {
        format!(
            "failed to create generated Vim build directory {}: {error}",
            auto_dir.display()
        )
    })?;

    let configure_script = vendor_src_dir.join("auto/configure");
    let srcdir_arg = format!("--srcdir={}", vendor_src_dir.display());
    let mut configure = Command::new("sh");
    configure.current_dir(&build_root);
    configure.arg(&configure_script);
    configure.arg(&srcdir_arg);
    configure.arg("--disable-gui");
    configure.arg("--without-x");
    configure.arg("--with-features=normal");
    configure.arg("--disable-selinux");
    configure.arg("--disable-smack");
    configure.arg("--disable-xattr");
    configure.arg("--disable-xim");
    configure.arg("--disable-netbeans");
    configure.arg("--disable-channel");
    configure.arg("--disable-terminal");
    configure.arg("--enable-gui=no");
    configure.arg("--enable-socketserver=no");
    configure.arg("--enable-cscope=no");
    configure.arg(format!("--with-modified-by={VIM_MODIFIED_BY}"));
    run_command(&mut configure, "generate Vim config headers")?;

    let generated_config_h = build_root.join("auto/config.h");
    let staged_auto_dir = staged_vendor_src_root.join("auto");
    fs::create_dir_all(&staged_auto_dir).map_err(|error| {
        format!(
            "failed to create staged Vim auto directory {}: {error}",
            staged_auto_dir.display()
        )
    })?;
    fs::copy(&generated_config_h, staged_auto_dir.join("config.h")).map_err(|error| {
        format!(
            "failed to stage generated config.h into {}: {error}",
            staged_auto_dir.display()
        )
    })?;

    let vendor_xdiff_dir = vendor_src_dir.join("xdiff");
    let staged_xdiff_dir = staged_vendor_src_root.join("xdiff");
    copy_directory_recursively(&vendor_xdiff_dir, &staged_xdiff_dir)?;

    generate_osdef_header(vendor_src_dir, &build_root)?;

    let vim_paths = prepare_bundled_runtime(repo_root, out_dir, &metadata.tag)?;
    write_generated_pathdef(&auto_dir.join("pathdef.c"), &vim_paths)?;

    Ok(GeneratedVimBuildArtifacts {
        include_root: build_root,
        generated_sources: vec![auto_dir.join("pathdef.c")],
        staged_vendor_src_root,
    })
}

fn copy_directory_recursively(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| {
        format!(
            "failed to create staged directory {}: {error}",
            destination.display()
        )
    })?;

    for entry in fs::read_dir(source)
        .map_err(|error| format!("failed to read directory {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read directory entry in {}: {error}",
                source.display()
            )
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| {
            format!(
                "failed to read file type for {}: {error}",
                source_path.display()
            )
        })?;

        if file_type.is_dir() {
            copy_directory_recursively(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).map_err(|error| {
                format!(
                    "failed to copy {} to {}: {error}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }

    Ok(())
}

struct VimPaths {
    vim_dir: String,
    vimruntime_dir: String,
}

fn refresh_prebuilt_runtime_contract(
    repo_root: &Path,
    out_dir: &Path,
    upstream_tag: &str,
) -> Result<(), String> {
    let vim_paths = prepare_bundled_runtime(repo_root, out_dir, upstream_tag)?;
    write_generated_pathdef(
        &out_dir.join("vim_build").join("auto").join("pathdef.c"),
        &vim_paths,
    )
}

fn prepare_bundled_runtime(
    repo_root: &Path,
    out_dir: &Path,
    upstream_tag: &str,
) -> Result<VimPaths, String> {
    let version_dir_name = vim_version_dir_name(upstream_tag)?;
    let bundled_vim_dir = out_dir.join("share").join("vim");
    let bundled_vimruntime_dir = bundled_vim_dir.join(&version_dir_name);
    let vendored_runtime_dir = repo_root.join("vendor/upstream/vim/runtime");

    if bundled_vimruntime_dir.exists() {
        fs::remove_dir_all(&bundled_vimruntime_dir).map_err(|error| {
            format!(
                "failed to remove stale bundled runtime directory {}: {error}",
                bundled_vimruntime_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&bundled_vim_dir).map_err(|error| {
        format!(
            "failed to create bundled Vim runtime root {}: {error}",
            bundled_vim_dir.display()
        )
    })?;
    copy_directory_recursively(&vendored_runtime_dir, &bundled_vimruntime_dir)?;

    let vim_dir = bundled_vim_dir.to_string_lossy().into_owned();
    let vimruntime_dir = bundled_vimruntime_dir.to_string_lossy().into_owned();

    Ok(VimPaths {
        vim_dir,
        vimruntime_dir,
    })
}

fn vim_version_dir_name(tag: &str) -> Result<String, String> {
    let version = tag
        .strip_prefix('v')
        .ok_or_else(|| format!("metadata tag {tag} does not start with 'v'"))?;

    let segments: Vec<&str> = version.split('.').collect();
    if segments.len() < 2 {
        return Err(format!(
            "metadata tag {tag} does not contain major.minor version"
        ));
    }

    let major = segments[0];
    let minor = segments[1];

    Ok(format!("vim{major}{minor}"))
}

fn write_generated_pathdef(path: &Path, vim_paths: &VimPaths) -> Result<(), String> {
    let content = format!(
        "#include \"vim.h\"\n\n\
         char_u *default_vim_dir = (char_u *)\"{vim_dir}\";\n\
         char_u *default_vimruntime_dir = (char_u *)\"{vimruntime_dir}\";\n\
         char_u *all_cflags = (char_u *)\"\";\n\
         char_u *all_lflags = (char_u *)\"\";\n\
         char_u *compiled_user = (char_u *)\"vim-core-rs\";\n\
         char_u *compiled_sys = (char_u *)\"vim-core-rs\";\n",
        vim_dir = vim_paths.vim_dir,
        vimruntime_dir = vim_paths.vimruntime_dir,
    );

    fs::write(path, content).map_err(|error| {
        format!(
            "failed to write generated pathdef source {}: {error}",
            path.display()
        )
    })
}

fn generate_osdef_header(vendor_src_dir: &Path, build_root: &Path) -> Result<(), String> {
    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let mut command = Command::new("sh");
    command.current_dir(build_root);
    command.arg(vendor_src_dir.join("osdef.sh"));
    command.env("srcdir", vendor_src_dir);
    command.env("CC", cc);
    run_command(&mut command, "generate Vim osdef header")
}

fn combine_archives(out_dir: &Path, archives: &[PathBuf]) -> Result<PathBuf, String> {
    combine_archives_named(out_dir, "libvimcore.a", archives)
}

fn combine_archives_named(
    out_dir: &Path,
    output_name: &str,
    archives: &[PathBuf],
) -> Result<PathBuf, String> {
    let ar = env::var("AR").unwrap_or_else(|_| "ar".to_string());
    let final_archive = out_dir.join(output_name);
    if final_archive.exists() {
        fs::remove_file(&final_archive).map_err(|error| {
            format!(
                "failed to remove existing archive {}: {error}",
                final_archive.display()
            )
        })?;
    }

    let mut object_files = Vec::new();
    for archive in archives {
        let extract_dir = out_dir.join(
            archive
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("archive_members"),
        );

        if extract_dir.exists() {
            fs::remove_dir_all(&extract_dir).map_err(|error| {
                format!(
                    "failed to clean archive extract dir {}: {error}",
                    extract_dir.display()
                )
            })?;
        }
        fs::create_dir_all(&extract_dir).map_err(|error| {
            format!(
                "failed to create archive extract dir {}: {error}",
                extract_dir.display()
            )
        })?;

        run_command(
            Command::new(&ar)
                .current_dir(&extract_dir)
                .arg("x")
                .arg(archive),
            &format!("extract archive {}", archive.display()),
        )?;

        for entry in fs::read_dir(&extract_dir).map_err(|error| {
            format!(
                "failed to read archive extract dir {}: {error}",
                extract_dir.display()
            )
        })? {
            let entry = entry.map_err(|error| {
                format!(
                    "failed to read archive member from {}: {error}",
                    extract_dir.display()
                )
            })?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("o") {
                object_files.push(path);
            }
        }
    }

    if object_files.is_empty() {
        return Err("combined archive does not contain object files".to_string());
    }

    let mut command = Command::new(&ar);
    command.arg("crus").arg(&final_archive);
    for object_file in &object_files {
        command.arg(object_file);
    }
    run_command(&mut command, "create combined vimcore archive")?;

    Ok(final_archive)
}

fn generate_bindings(bridge_header: &Path, out_dir: &Path) -> Result<(), String> {
    let bindings = bindgen::Builder::default()
        .header(bridge_header.to_string_lossy())
        .allowlist_function("vim_bridge_.*")
        .allowlist_type("vim_(bridge|core|host_action|runtime_backend_identity)_.*")
        .allowlist_var("VIM_(CORE|HOST_ACTION)_.*")
        .generate()
        .map_err(|error| {
            format!(
                "failed to generate bindings from {}: {error}",
                bridge_header.display()
            )
        })?;

    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .map_err(|error| format!("failed to write generated bindings: {error}"))?;

    Ok(())
}

fn run_command(command: &mut Command, action: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to {action}: {error}"))?;

    if output.status.success() {
        return Ok(());
    }

    Err(format!(
        "failed to {action}: status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}
