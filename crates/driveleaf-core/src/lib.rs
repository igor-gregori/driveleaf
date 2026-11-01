//! Disk usage model and the scanners that fill it.
//!
//! [`mft_tree::build_model`] turns an MFT scan into a [`model::Model`], and
//! [`walk::walk`] builds the same model folder by folder when the MFT is out of
//! reach. Listings, searches and the treemap all read from that model.

#![forbid(unsafe_code)]

pub mod categories;
pub mod mft_tree;
pub mod model;
pub mod query;
pub mod safety;
pub mod scanner;
pub mod walk;
