//! Moca Warp: local fork of Warp with no Warp account or servers.

/// When true, UI that invites the user to sign in / sign up for a Warp account is hidden.
/// Every call site is marked with a `// moca:` comment so it is easy to find when
/// merging upstream changes.
pub const ACCOUNTS_DISABLED: bool = true;
