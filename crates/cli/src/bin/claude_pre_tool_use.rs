//! Native Claude command hook: no MCP response, identity synthesis, or approval output.
use clap::Parser;
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use serde::Deserialize;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use tect_domain::{Error, Result};
use tect_host::native_identity::{
    CLAUDE_ATTESTATION_MAX_BYTES, CLAUDE_ATTESTATION_MAX_TTL_MS, ClaudeToolAttestation,
    claude_attestation_filename, open_claude_attestation_directory, unix_time_ms,
};
use uuid::Uuid;

#[derive(Parser)]
struct Options {
    #[arg(long)]
    destination_host_id: Uuid,
    #[arg(long)]
    server_alias: String,
    #[arg(long)]
    context_dir: PathBuf,
}

// Official hooks may include cwd, transcript_path, permission_mode and other fields.
#[derive(Deserialize)]
struct HookInput {
    hook_event_name: String,
    session_id: String,
    tool_use_id: String,
    tool_name: String,
    tool_input: Value,
}

pub(super) fn run(arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<()> {
    let options = Options::try_parse_from(arguments).map_err(|_| Error::InvalidConfiguration)?;
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(CLAUDE_ATTESTATION_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidNativeSession)?;
    let now = unix_time_ms()?;
    let record = parse(&bytes, &options, now)?;
    publish(&options, &record, now)
}

fn parse(bytes: &[u8], options: &Options, now: u64) -> Result<ClaudeToolAttestation> {
    if bytes.len() > CLAUDE_ATTESTATION_MAX_BYTES {
        return Err(Error::InvalidNativeSession);
    }
    let input: HookInput =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidNativeSession)?;
    if input.hook_event_name != "PreToolUse" {
        return Err(Error::InvalidNativeSession);
    }
    ClaudeToolAttestation::new(
        input.session_id,
        input.tool_use_id,
        input.tool_name,
        input.tool_input,
        options.server_alias.clone(),
        options.destination_host_id,
        now,
        now.checked_add(CLAUDE_ATTESTATION_MAX_TTL_MS)
            .ok_or(Error::InvalidNativeSession)?,
    )
}

fn read_existing(
    directory: &rustix::fd::OwnedFd,
    name: &str,
) -> Result<Option<ClaudeToolAttestation>> {
    let fd = match fs::openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(_) => return Err(Error::InvalidNativeSession),
    };
    let mut file = File::from(fd);
    let before = fs::fstat(&file).map_err(|_| Error::InvalidNativeSession)?;
    validate_file(&before)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(CLAUDE_ATTESTATION_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidNativeSession)?;
    let after = fs::fstat(&file).map_err(|_| Error::InvalidNativeSession)?;
    let named = fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| Error::InvalidNativeSession)?;
    validate_file(&after)?;
    validate_file(&named)?;
    if bytes.len() > CLAUDE_ATTESTATION_MAX_BYTES
        || bytes.len() as u64 != before.st_size as u64
        || !same_file(&before, &after)
        || !same_file(&before, &named)
    {
        return Err(Error::InvalidNativeSession);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Error::InvalidNativeSession)
}

fn validate_file(stat: &fs::Stat) -> Result<()> {
    if !FileType::from_raw_mode(stat.st_mode).is_file()
        || stat.st_mode & 0o7777 != 0o600
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_nlink != 1
        || stat.st_size < 0
        || stat.st_size as u64 > CLAUDE_ATTESTATION_MAX_BYTES as u64
    {
        return Err(Error::InvalidNativeSession);
    }
    Ok(())
}

fn same_file(a: &fs::Stat, b: &fs::Stat) -> bool {
    a.st_dev == b.st_dev
        && a.st_ino == b.st_ino
        && a.st_size == b.st_size
        && a.st_mode == b.st_mode
        && a.st_uid == b.st_uid
        && a.st_nlink == b.st_nlink
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}

fn identical_retry(
    existing: &ClaudeToolAttestation,
    incoming: &ClaudeToolAttestation,
    now: u64,
) -> Result<()> {
    existing.validate_at(&incoming.server_alias, incoming.destination_host_id, now)?;
    let mut comparison = incoming.clone();
    comparison.issued_at_unix_ms = existing.issued_at_unix_ms;
    comparison.expires_at_unix_ms = existing.expires_at_unix_ms;
    if existing != &comparison {
        return Err(Error::InvalidNativeSession);
    }
    Ok(())
}

fn publish(options: &Options, record: &ClaudeToolAttestation, now: u64) -> Result<()> {
    let directory = open_claude_attestation_directory(&options.context_dir)?;
    let name = claude_attestation_filename(&record.tool_use_id)?;
    if let Some(existing) = read_existing(&directory, &name)? {
        identical_retry(&existing, record, now)?;
        fs::fsync(&directory).map_err(|_| Error::InvalidNativeSession)?;
        return Ok(());
    }
    let bytes = serde_json::to_vec(record).map_err(|_| Error::InvalidNativeSession)?;
    if bytes.len() > CLAUDE_ATTESTATION_MAX_BYTES {
        return Err(Error::InvalidNativeSession);
    }
    // Randomness names only an exclusive temporary file, never a native identity.
    let temporary = format!(".pending-{}", Uuid::new_v4());
    let fd = fs::openat(
        &directory,
        &temporary,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|_| Error::InvalidNativeSession)?;
    let mut file = File::from(fd);
    let result = (|| {
        fs::fchmod(&file, Mode::from_raw_mode(0o600)).map_err(|_| Error::InvalidNativeSession)?;
        file.write_all(&bytes)
            .map_err(|_| Error::InvalidNativeSession)?;
        file.sync_all().map_err(|_| Error::InvalidNativeSession)?;
        validate_file(&fs::fstat(&file).map_err(|_| Error::InvalidNativeSession)?)?;
        match fs::linkat(&directory, &temporary, &directory, &name, AtFlags::empty()) {
            Ok(()) => {}
            Err(rustix::io::Errno::EXIST) => {
                let existing =
                    read_existing(&directory, &name)?.ok_or(Error::InvalidNativeSession)?;
                identical_retry(&existing, record, now)?;
            }
            Err(_) => return Err(Error::InvalidNativeSession),
        }
        Ok(())
    })();
    let removed = fs::unlinkat(&directory, &temporary, AtFlags::empty())
        .map_err(|_| Error::InvalidNativeSession);
    result?;
    removed?;
    // The final file has one link only after the temporary link is removed.
    let existing = read_existing(&directory, &name)?.ok_or(Error::InvalidNativeSession)?;
    identical_retry(&existing, record, now)?;
    fs::fsync(&directory).map_err(|_| Error::InvalidNativeSession)
}

#[cfg(test)]
mod tests;
