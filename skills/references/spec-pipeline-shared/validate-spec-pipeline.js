"use strict";

const fs = require("node:fs");
const path = require("node:path");

const REQUIRED_FILES = Object.freeze([
  "requirements-ledger.json",
  "decision-traceability.json",
  "acceptance-obligations.json",
  "reconciliation-closure.json",
  "synthesis-traceability.json"
]);
const RECONCILIATION_FILES = Object.freeze(REQUIRED_FILES.slice(0, -1));
const VALIDATION_STAGES = new Set(["reconciliation", "synthesis"]);
const MODALITIES = new Set(["MUST", "SHOULD", "MAY"]);
const REQUIREMENT_STATUSES = new Set(["covered", "deferred", "ambiguous", "untestable", "gap"]);
const CLOSURE_MODES = new Set(["findings-resolution", "closure-audit"]);

function validateSpecPipeline(options = {}) {
  const rootPath = path.resolve(options.rootPath || process.cwd());
  const fsImpl = options.fsImpl || fs;
  const errors = [];
  const documents = {};
  const stage = options.stage || "synthesis";

  function addError(code, artifactPath, message) {
    errors.push({ code, path: artifactPath, message });
  }

  if (!VALIDATION_STAGES.has(stage)) {
    addError("VALIDATION_STAGE_INVALID", "stage", "stage must be reconciliation or synthesis");
  }
  const requiredFiles = stage === "reconciliation" ? RECONCILIATION_FILES : REQUIRED_FILES;

  for (const fileName of requiredFiles) {
    const filePath = path.join(rootPath, fileName);
    if (!fsImpl.existsSync(filePath)) {
      addError("ARTIFACT_MISSING", fileName, `required artifact is missing: ${fileName}`);
      continue;
    }
    try {
      documents[fileName] = JSON.parse(fsImpl.readFileSync(filePath, "utf8"));
    } catch (error) {
      addError("ARTIFACT_JSON_INVALID", fileName, `cannot parse ${fileName}: ${error.message}`);
    }
  }

  const counts = {
    requirements: 0,
    obligations: 0,
    mappedRequirements: 0,
    synthesizedRequirements: 0
  };
  if (Object.keys(documents).length !== requiredFiles.length || !VALIDATION_STAGES.has(stage)) {
    return { valid: false, stage, errors, counts };
  }

  const ledger = documents["requirements-ledger.json"];
  const decisionTraceability = documents["decision-traceability.json"];
  const acceptance = documents["acceptance-obligations.json"];
  const closure = documents["reconciliation-closure.json"];
  const synthesis = documents["synthesis-traceability.json"] || null;

  for (const [fileName, document] of Object.entries(documents)) {
    if (!isRecord(document)) {
      addError("ARTIFACT_SHAPE_INVALID", fileName, `${fileName} must contain a JSON object`);
    } else if (document.schemaVersion !== "1.0") {
      addError("SCHEMA_VERSION_UNSUPPORTED", `${fileName}.schemaVersion`, "schemaVersion must be 1.0");
    }
  }

  const sourceRequirementIds = arrayAt(ledger, "sourceRequirementIds", "requirements-ledger.json", addError);
  const requirementRows = arrayAt(ledger, "requirements", "requirements-ledger.json", addError);
  const decisionRows = arrayAt(decisionTraceability, "requirements", "decision-traceability.json", addError);
  const obligationRows = arrayAt(acceptance, "obligations", "acceptance-obligations.json", addError);
  const synthesisRows = stage === "synthesis"
    ? arrayAt(synthesis, "requirements", "synthesis-traceability.json", addError)
    : [];
  const unresolvedIds = arrayAt(closure, "unresolvedRequirementIds", "reconciliation-closure.json", addError);

  counts.requirements = requirementRows.length;
  counts.obligations = obligationRows.length;
  counts.mappedRequirements = decisionRows.length;
  counts.synthesizedRequirements = synthesisRows.length;

  const sourceIds = uniqueStringSet(sourceRequirementIds, "requirements-ledger.json.sourceRequirementIds", "SOURCE_REQUIREMENT_ID", addError);
  const requirementsById = indexRows(requirementRows, "id", "requirements-ledger.json.requirements", "REQUIREMENT", addError);
  const decisionsByRequirement = indexRows(
    decisionRows,
    "requirementId",
    "decision-traceability.json.requirements",
    "DECISION_TRACE",
    addError
  );
  const obligationsById = indexRows(obligationRows, "id", "acceptance-obligations.json.obligations", "OBLIGATION", addError);
  const synthesisByRequirement = indexRows(
    synthesisRows,
    "requirementId",
    "synthesis-traceability.json.requirements",
    "SYNTHESIS_TRACE",
    addError
  );

  for (const sourceId of sourceIds) {
    if (!requirementsById.has(sourceId)) {
      addError(
        "SOURCE_REQUIREMENT_MISSING",
        "requirements-ledger.json.requirements",
        `source requirement ${sourceId} has no ledger row`
      );
    }
  }

  const activeRequirementIds = new Set();
  for (const [requirementId, row] of requirementsById) {
    const rowPath = `requirements-ledger.json.requirements[${requirementId}]`;
    if (!sourceIds.has(requirementId)) {
      addError("REQUIREMENT_NOT_IN_SOURCE_INVENTORY", rowPath, `${requirementId} is absent from sourceRequirementIds`);
    }
    requireString(row, "sourceRef", "REQUIREMENT_SOURCE_REF_MISSING", rowPath, addError);
    requireString(row, "text", "REQUIREMENT_TEXT_MISSING", rowPath, addError);
    if (!MODALITIES.has(row.modality)) {
      addError("REQUIREMENT_MODALITY_INVALID", `${rowPath}.modality`, `${requirementId} must use MUST, SHOULD, or MAY`);
    }
    if (row.scope !== "in" && row.scope !== "out") {
      addError("REQUIREMENT_SCOPE_INVALID", `${rowPath}.scope`, `${requirementId} must use scope in or out`);
    }
    if (!REQUIREMENT_STATUSES.has(row.status)) {
      addError("REQUIREMENT_STATUS_INVALID", `${rowPath}.status`, `${requirementId} has unsupported status`);
    }
    if (row.scope !== "in") {
      continue;
    }
    requireString(row, "owner", "REQUIREMENT_OWNER_MISSING", rowPath, addError);
    requireStringArray(row, "observableOutcomes", "REQUIREMENT_OBSERVABLE_MISSING", rowPath, addError);
    requireStringArray(row, "negativeCases", "REQUIREMENT_NEGATIVE_CASE_MISSING", rowPath, addError);

    if (row.status === "deferred") {
      const deferral = row.deferral;
      if (!isRecord(deferral)
        || !isNonEmptyString(deferral.approvedBy)
        || !isNonEmptyString(deferral.rationale)
        || !isNonEmptyString(deferral.sourceRef)) {
        addError(
          "DEFERRAL_AUTHORITY_MISSING",
          `${rowPath}.deferral`,
          `${requirementId} needs approvedBy, rationale, and sourceRef for deferral`
        );
      }
      continue;
    }
    if (row.status !== "covered") {
      addError("REQUIREMENT_NOT_CLOSED", `${rowPath}.status`, `${requirementId} is ${row.status || "unclassified"}`);
    }
    activeRequirementIds.add(requirementId);
  }

  for (const [requirementId, row] of decisionsByRequirement) {
    const rowPath = `decision-traceability.json.requirements[${requirementId}]`;
    if (!requirementsById.has(requirementId)) {
      addError("UNKNOWN_REQUIREMENT_REFERENCE", `${rowPath}.requirementId`, `unknown requirement ${requirementId}`);
    }
    requireStringArray(row, "decisionRefs", "DECISION_REFERENCE_MISSING", rowPath, addError);
    requireStringArray(row, "acceptanceObligationIds", "ACCEPTANCE_MAPPING_MISSING", rowPath, addError);
    for (const obligationId of stringArray(row.acceptanceObligationIds)) {
      if (!obligationsById.has(obligationId)) {
        addError(
          "UNKNOWN_ACCEPTANCE_OBLIGATION",
          `${rowPath}.acceptanceObligationIds`,
          `${requirementId} references unknown obligation ${obligationId}`
        );
      }
    }
  }

  for (const requirementId of activeRequirementIds) {
    if (!decisionsByRequirement.has(requirementId)) {
      addError(
        "DECISION_TRACE_MISSING",
        "decision-traceability.json.requirements",
        `${requirementId} has no decision trace`
      );
    }
  }

  const obligationKindsByRequirement = new Map();
  for (const [obligationId, row] of obligationsById) {
    const rowPath = `acceptance-obligations.json.obligations[${obligationId}]`;
    const linkedRequirementIds = requireStringArray(
      row,
      "requirementIds",
      "OBLIGATION_REQUIREMENT_MISSING",
      rowPath,
      addError
    );
    if (row.kind !== "positive" && row.kind !== "negative") {
      addError("OBLIGATION_KIND_INVALID", `${rowPath}.kind`, `${obligationId} must be positive or negative`);
    }
    requireString(row, "owner", "OBLIGATION_OWNER_MISSING", rowPath, addError);
    requireString(row, "observable", "OBLIGATION_OBSERVABLE_MISSING", rowPath, addError);
    requireString(row, "verification", "OBLIGATION_VERIFICATION_MISSING", rowPath, addError);
    for (const requirementId of linkedRequirementIds) {
      if (!requirementsById.has(requirementId)) {
        addError("UNKNOWN_REQUIREMENT_REFERENCE", `${rowPath}.requirementIds`, `${obligationId} links unknown ${requirementId}`);
        continue;
      }
      if (!obligationKindsByRequirement.has(requirementId)) {
        obligationKindsByRequirement.set(requirementId, new Set());
      }
      obligationKindsByRequirement.get(requirementId).add(row.kind);
    }
  }

  for (const requirementId of activeRequirementIds) {
    const kinds = obligationKindsByRequirement.get(requirementId) || new Set();
    if (!kinds.has("positive")) {
      addError("POSITIVE_OBLIGATION_MISSING", "acceptance-obligations.json.obligations", `${requirementId} lacks a positive obligation`);
    }
    if (!kinds.has("negative")) {
      addError("NEGATIVE_OBLIGATION_MISSING", "acceptance-obligations.json.obligations", `${requirementId} lacks a negative obligation`);
    }
  }

  if (stage === "synthesis") {
    for (const [requirementId, row] of synthesisByRequirement) {
      const rowPath = `synthesis-traceability.json.requirements[${requirementId}]`;
      const requirement = requirementsById.get(requirementId);
      if (!requirement) {
        addError("UNKNOWN_REQUIREMENT_REFERENCE", `${rowPath}.requirementId`, `unknown requirement ${requirementId}`);
        continue;
      }
      if (row.modality !== requirement.modality) {
        addError(
          "SYNTHESIS_MODALITY_MISMATCH",
          `${rowPath}.modality`,
          `${requirementId} changed ${requirement.modality} to ${row.modality || "missing"}`
        );
      }
      requireStringArray(row, "sectionRefs", "SYNTHESIS_SECTION_MISSING", rowPath, addError);
    }

    for (const requirementId of activeRequirementIds) {
      if (!synthesisByRequirement.has(requirementId)) {
        addError(
          "SYNTHESIS_TRACE_MISSING",
          "synthesis-traceability.json.requirements",
          `${requirementId} has no synthesis trace`
        );
      }
    }
  }

  if (!CLOSURE_MODES.has(closure.mode)) {
    addError("RECONCILIATION_MODE_INVALID", "reconciliation-closure.json.mode", "mode must be findings-resolution or closure-audit");
  }
  if (closure.completionClaim !== "ready" && closure.completionClaim !== "blocked") {
    addError("COMPLETION_CLAIM_INVALID", "reconciliation-closure.json.completionClaim", "completionClaim must be ready or blocked");
  }
  if (closure.completionClaim === "ready" && unresolvedIds.length > 0) {
    addError(
      "COMPLETION_WITH_UNRESOLVED_REQUIREMENTS",
      "reconciliation-closure.json.unresolvedRequirementIds",
      "ready completion claim cannot contain unresolved requirement IDs"
    );
  }
  for (const requirementId of stringArray(unresolvedIds)) {
    if (!sourceIds.has(requirementId)) {
      addError(
        "UNKNOWN_UNRESOLVED_REQUIREMENT",
        "reconciliation-closure.json.unresolvedRequirementIds",
        `unknown unresolved requirement ${requirementId}`
      );
    }
  }
  if (stage === "synthesis" && synthesis.completionClaim !== "ready" && synthesis.completionClaim !== "blocked") {
    addError("SYNTHESIS_COMPLETION_CLAIM_INVALID", "synthesis-traceability.json.completionClaim", "completionClaim must be ready or blocked");
  }

  return { valid: errors.length === 0, stage, errors, counts };
}

function arrayAt(record, key, artifactPath, addError) {
  if (!isRecord(record) || !Array.isArray(record[key])) {
    addError("ARTIFACT_ARRAY_MISSING", `${artifactPath}.${key}`, `${key} must be an array`);
    return [];
  }
  return record[key];
}

function indexRows(rows, key, artifactPath, prefix, addError) {
  const index = new Map();
  rows.forEach((row, position) => {
    const rowPath = `${artifactPath}[${position}]`;
    if (!isRecord(row) || !isNonEmptyString(row[key])) {
      addError(`${prefix}_ID_MISSING`, `${rowPath}.${key}`, `${key} must be a non-empty string`);
      return;
    }
    if (index.has(row[key])) {
      addError(`${prefix}_ID_DUPLICATE`, `${rowPath}.${key}`, `duplicate ${key}: ${row[key]}`);
      return;
    }
    index.set(row[key], row);
  });
  return index;
}

function uniqueStringSet(values, artifactPath, prefix, addError) {
  const result = new Set();
  values.forEach((value, position) => {
    if (!isNonEmptyString(value)) {
      addError(`${prefix}_INVALID`, `${artifactPath}[${position}]`, "value must be a non-empty string");
      return;
    }
    if (result.has(value)) {
      addError(`${prefix}_DUPLICATE`, `${artifactPath}[${position}]`, `duplicate ID: ${value}`);
      return;
    }
    result.add(value);
  });
  return result;
}

function requireString(record, key, code, artifactPath, addError) {
  if (!isRecord(record) || !isNonEmptyString(record[key])) {
    addError(code, `${artifactPath}.${key}`, `${key} must be a non-empty string`);
    return "";
  }
  return record[key];
}

function requireStringArray(record, key, code, artifactPath, addError) {
  const value = isRecord(record) ? record[key] : null;
  const valid = Array.isArray(value) && value.length > 0 && value.every(isNonEmptyString);
  if (!valid) {
    addError(code, `${artifactPath}.${key}`, `${key} must contain non-empty strings`);
    return [];
  }
  return value;
}

function stringArray(value) {
  return Array.isArray(value) ? value.filter(isNonEmptyString) : [];
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isNonEmptyString(value) {
  return typeof value === "string" && value.trim().length > 0;
}

if (require.main === module) {
  const rootPath = process.argv[2];
  const stageFlagIndex = process.argv.indexOf("--stage");
  const stage = stageFlagIndex >= 0 ? process.argv[stageFlagIndex + 1] : "synthesis";
  if (!rootPath) {
    process.stdout.write(`${JSON.stringify({
      valid: false,
      errors: [{
        code: "ROOT_PATH_REQUIRED",
        path: "argv[2]",
        message: "usage: node validate-spec-pipeline.js <artifact-directory> [--stage reconciliation|synthesis]"
      }]
    }, null, 2)}\n`);
    process.exitCode = 2;
  } else {
    const result = validateSpecPipeline({ rootPath, stage });
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    process.exitCode = result.valid ? 0 : 1;
  }
}

module.exports = {
  RECONCILIATION_FILES,
  REQUIRED_FILES,
  validateSpecPipeline
};
