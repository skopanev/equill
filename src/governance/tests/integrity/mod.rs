//! What happens when a store is interrupted, tampered with, or written by a
//! build that knew more than this one.
mod forward_compat;
mod recovery;
mod tampering;
