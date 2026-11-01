// The dam: one application with one small program per job, so that each process holds only
// the code of its own job. No network and no model anywhere.
pub mod folds;
pub mod graph;
pub mod json;
pub mod loopguard;
pub mod ponytail;
pub mod scan;
pub mod squeeze;
