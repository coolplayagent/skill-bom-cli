//! Explicit lock/installed views and SPDX 2.3 mapping.
use crate::domain::*;
use crate::installer::Verification;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bom {
    pub schema_version: u32,
    pub generator: String,
    pub generated_at: String,
    pub view: String,
    pub project: String,
    pub manifest_digest: String,
    pub lock_digest: String,
    pub roots: BTreeMap<String, Edge>,
    pub packages: BTreeMap<String, LockedPackage>,
    pub verification: Option<Verification>,
    pub remote_status_refreshed: bool,
}
pub fn native(
    project: &str,
    lock: &Lock,
    generated_at: String,
    verification: Option<Verification>,
) -> Result<Bom> {
    lock.validate()?;
    Ok(Bom {
        schema_version: SCHEMA,
        generator: format!("skill-bom/{}", env!("CARGO_PKG_VERSION")),
        generated_at,
        view: if verification.is_some() {
            "installed"
        } else {
            "lock"
        }
        .into(),
        project: project.into(),
        manifest_digest: lock.manifest_digest.clone(),
        lock_digest: json_digest(lock)?,
        roots: lock.roots.clone(),
        packages: lock.packages.clone(),
        verification,
        remote_status_refreshed: false,
    })
}
pub fn spdx(bom: &Bom) -> Result<Value> {
    let mut packages = vec![
        json!({"SPDXID":"SPDXRef-Root","name":bom.project,"downloadLocation":"NOASSERTION","filesAnalyzed":false,"licenseConcluded":"NOASSERTION","licenseDeclared":"NOASSERTION","copyrightText":"NOASSERTION"}),
    ];
    let ids: BTreeMap<_, _> = bom
        .packages
        .keys()
        .enumerate()
        .map(|(i, id)| (id, format!("SPDXRef-Skill-{}", i + 1)))
        .collect();
    let mut relationships = vec![
        json!({"spdxElementId":"SPDXRef-DOCUMENT","relationshipType":"DESCRIBES","relatedSpdxElement":"SPDXRef-Root"}),
    ];
    let mut edges = BTreeSet::new();
    for edge in bom.roots.values() {
        edges.insert(("SPDXRef-Root".to_string(), ids[&edge.package].clone()));
    }
    for (id, p) in &bom.packages {
        let spdx_id = &ids[id];
        // Arbitrary declared license text is retained in comment; it is not assumed to be an SPDX expression.
        let license = p
            .metadata
            .license
            .as_deref()
            .filter(|v| {
                matches!(
                    *v,
                    "MIT"
                        | "Apache-2.0"
                        | "BSD-2-Clause"
                        | "BSD-3-Clause"
                        | "ISC"
                        | "MPL-2.0"
                        | "GPL-3.0-only"
                        | "GPL-2.0-only"
                        | "CC0-1.0"
                        | "CC-BY-4.0"
                        | "Unlicense"
                )
            })
            .unwrap_or("NOASSERTION");
        let comment = serde_json::to_string(
            &json!({"skillBom":{"packageId":id,"view":bom.view,"treeDigest":{"algorithm":p.tree_algorithm,"value":p.tree_sha256,"scope":"normalized-content-tree"},"metadata":p.metadata,"evidence":p.evidence,"source":p.source,"requests":p.dependencies,"verification":bom.verification.as_ref().and_then(|v|v.packages.get(id)),"remoteStatusRefreshed":false}}),
        )?;
        let mut package = json!({"SPDXID":spdx_id,"name":p.metadata.name,"downloadLocation":p.source.location(),"filesAnalyzed":false,"licenseConcluded":"NOASSERTION","licenseDeclared":license,"copyrightText":"NOASSERTION","comment":comment});
        package["versionInfo"] = json!(p.candidate.display());
        if let Some(sha) = &p.evidence.archive_sha256 {
            package["checksums"] = json!([{"algorithm":"SHA256","checksumValue":sha}]);
        }
        if let Source::Clawhub { owner, .. } = &p.source {
            package["originator"] = json!(format!("Organization: {owner}"));
        }
        packages.push(package);
        for edge in p.dependencies.values() {
            edges.insert((spdx_id.clone(), ids[&edge.package].clone()));
        }
    }
    for (a, b) in edges {
        relationships.push(
            json!({"spdxElementId":a,"relationshipType":"DEPENDS_ON","relatedSpdxElement":b}),
        );
    }
    let value = json!({"spdxVersion":"SPDX-2.3","dataLicense":"CC0-1.0","SPDXID":"SPDXRef-DOCUMENT","name":format!("{} Skill BOM ({})",bom.project,bom.view),"documentNamespace":format!("https://spdx.org/spdxdocs/skill-bom-{}-{}",bom.lock_digest,digest(format!("{}:{}",bom.generated_at,bom.view).as_bytes())),"creationInfo":{"creators":[format!("Tool: {}",bom.generator)],"created":bom.generated_at},"comment":serde_json::to_string(&json!({"view":bom.view,"verification":bom.verification,"manifestDigest":bom.manifest_digest,"lockDigest":bom.lock_digest}))?,"packages":packages,"relationships":relationships});
    validate_references(&value)?;
    Ok(value)
}
pub fn validate_references(doc: &Value) -> Result<()> {
    let mut ids = BTreeSet::from(["SPDXRef-DOCUMENT"]);
    for p in doc["packages"]
        .as_array()
        .ok_or_else(|| Error::new("SPDX", "Missing packages", 1))?
    {
        let id = p["SPDXID"]
            .as_str()
            .ok_or_else(|| Error::new("SPDX", "Missing SPDXID", 1))?;
        if !ids.insert(id) {
            return fail("SPDX", "Duplicate SPDXID");
        }
    }
    for r in doc["relationships"]
        .as_array()
        .ok_or_else(|| Error::new("SPDX", "Missing relationships", 1))?
    {
        for key in ["spdxElementId", "relatedSpdxElement"] {
            if !r[key].as_str().is_some_and(|id| ids.contains(id)) {
                return fail("SPDX", "Dangling relationship");
            }
        }
    }
    Ok(())
}
