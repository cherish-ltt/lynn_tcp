//! Message encoding/decoding module.
//!
//! This module provides low-level message encoding and decoding functions.
//! The primary encoding logic lives in `HandlerResult::get_response_data()` in the domain layer,
//! which calls these utilities for the wire format.
//!
//! TODO: Extract encoding/decoding primitives here in future refactors.
