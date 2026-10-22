//! repodata XML 渲染（primary / filelists / other / repomd）。

use super::super::{sha256_hex, xml_escape};
use super::{Dep, RpmPackage};

pub(super) fn render_primary(packages: &[RpmPackage]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<metadata xmlns=\"http://linux.duke.edu/metadata/common\" xmlns:rpm=\"http://linux.duke.edu/metadata/rpm\" packages=\"{}\">\n",
        packages.len()
    ));
    for package in packages {
        out.push_str("<package type=\"rpm\">\n");
        out.push_str(&format!("  <name>{}</name>\n", xml_escape(&package.name)));
        out.push_str(&format!("  <arch>{}</arch>\n", xml_escape(&package.arch)));
        out.push_str(&format!(
            "  <version epoch=\"{}\" ver=\"{}\" rel=\"{}\"/>\n",
            package.epoch,
            xml_escape(&package.version),
            xml_escape(&package.release)
        ));
        out.push_str(&format!(
            "  <checksum type=\"sha256\" pkgid=\"YES\">{}</checksum>\n",
            package.checksum
        ));
        out.push_str(&format!(
            "  <summary>{}</summary>\n",
            xml_escape(&package.summary)
        ));
        out.push_str(&format!(
            "  <description>{}</description>\n",
            xml_escape(&package.description)
        ));
        optional_element(&mut out, "packager", &package.packager);
        optional_element(&mut out, "url", &package.url);
        out.push_str(&format!(
            "  <time file=\"{}\" build=\"{}\"/>\n",
            package.mtime, package.buildtime
        ));
        out.push_str(&format!(
            "  <size package=\"{}\" installed=\"{}\" archive=\"{}\"/>\n",
            package.size, package.installed_size, package.archive_size
        ));
        out.push_str(&format!(
            "  <location href=\"{}\"/>\n",
            xml_escape(&package.location)
        ));
        out.push_str("  <format>\n");
        optional_element(&mut out, "rpm:license", &package.license);
        optional_element(&mut out, "rpm:vendor", &package.vendor);
        optional_element(&mut out, "rpm:group", &package.group);
        optional_element(&mut out, "rpm:buildhost", &package.buildhost);
        optional_element(&mut out, "rpm:sourcerpm", &package.sourcerpm);
        out.push_str(&format!(
            "    <rpm:header-range start=\"{}\" end=\"{}\"/>\n",
            package.header_start, package.header_end
        ));
        if !package.provides.is_empty() {
            out.push_str("    <rpm:provides>\n");
            for dep in &package.provides {
                out.push_str(&render_dep(dep));
            }
            out.push_str("    </rpm:provides>\n");
        }
        if !package.requires.is_empty() {
            out.push_str("    <rpm:requires>\n");
            for dep in &package.requires {
                out.push_str(&render_dep(dep));
            }
            out.push_str("    </rpm:requires>\n");
        }
        for file in package.files.iter().filter(|file| !file.is_dir) {
            out.push_str(&format!("    <file>{}</file>\n", xml_escape(&file.path)));
        }
        out.push_str("  </format>\n");
        out.push_str("</package>\n");
    }
    out.push_str("</metadata>\n");
    out
}

pub(super) fn render_filelists(packages: &[RpmPackage]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<filelists xmlns=\"http://linux.duke.edu/metadata/filelists\" packages=\"{}\">\n",
        packages.len()
    ));
    for package in packages {
        out.push_str(&format!(
            "<package pkgid=\"{}\" name=\"{}\" arch=\"{}\">\n",
            package.checksum,
            xml_escape(&package.name),
            xml_escape(&package.arch)
        ));
        out.push_str(&render_version(package));
        for file in &package.files {
            if file.is_dir {
                out.push_str(&format!(
                    "  <file type=\"dir\">{}</file>\n",
                    xml_escape(&file.path)
                ));
            } else {
                out.push_str(&format!("  <file>{}</file>\n", xml_escape(&file.path)));
            }
        }
        out.push_str("</package>\n");
    }
    out.push_str("</filelists>\n");
    out
}

pub(super) fn render_other(packages: &[RpmPackage]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<otherdata xmlns=\"http://linux.duke.edu/metadata/other\" packages=\"{}\">\n",
        packages.len()
    ));
    for package in packages {
        out.push_str(&format!(
            "<package pkgid=\"{}\" name=\"{}\" arch=\"{}\">\n",
            package.checksum,
            xml_escape(&package.name),
            xml_escape(&package.arch)
        ));
        out.push_str(&render_version(package));
        out.push_str("</package>\n");
    }
    out.push_str("</otherdata>\n");
    out
}

fn render_version(package: &RpmPackage) -> String {
    format!(
        "  <version epoch=\"{}\" ver=\"{}\" rel=\"{}\"/>\n",
        package.epoch,
        xml_escape(&package.version),
        xml_escape(&package.release)
    )
}

fn render_dep(dep: &Dep) -> String {
    let mut out = format!("      <rpm:entry name=\"{}\"", xml_escape(&dep.name));
    if !dep.flags.is_empty() {
        out.push_str(&format!(" flags=\"{}\"", dep.flags));
    }
    if !dep.ver.is_empty() {
        out.push_str(&format!(" epoch=\"{}\"", xml_escape(&dep.epoch)));
        out.push_str(&format!(" ver=\"{}\"", xml_escape(&dep.ver)));
        if !dep.rel.is_empty() {
            out.push_str(&format!(" rel=\"{}\"", xml_escape(&dep.rel)));
        }
    }
    out.push_str("/>\n");
    out
}

fn optional_element(out: &mut String, tag: &str, value: &str) {
    if !value.is_empty() {
        out.push_str(&format!("    <{tag}>{}</{tag}>\n", xml_escape(value)));
    }
}

/// repomd：三个 data 段（checksum / open-checksum / location / timestamp / size）。
pub(super) fn render_repomd(parts: &[(&str, &[u8], Vec<u8>, &str)], timestamp: u64) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(
        "<repomd xmlns=\"http://linux.duke.edu/metadata/repo\" xmlns:rpm=\"http://linux.duke.edu/metadata/rpm\">\n",
    );
    out.push_str(&format!("  <revision>{timestamp}</revision>\n"));
    for (kind, open, gz, href) in parts {
        out.push_str(&format!("  <data type=\"{kind}\">\n"));
        out.push_str(&format!(
            "    <checksum type=\"sha256\">{}</checksum>\n",
            sha256_hex(gz)
        ));
        out.push_str(&format!(
            "    <open-checksum type=\"sha256\">{}</open-checksum>\n",
            sha256_hex(open)
        ));
        out.push_str(&format!("    <location href=\"{}\"/>\n", xml_escape(href)));
        out.push_str(&format!("    <timestamp>{timestamp}</timestamp>\n"));
        out.push_str(&format!("    <size>{}</size>\n", gz.len()));
        out.push_str(&format!("    <open-size>{}</open-size>\n", open.len()));
        out.push_str("  </data>\n");
    }
    out.push_str("</repomd>\n");
    out
}
