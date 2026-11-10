//! Interval: a tracked block of time with optional tags and annotation.
//! Serialization format must match Interval.cpp::serialize() exactly:
//!   inc <ISOstart>Z [- <ISOend>Z] [# tag1 "tag with spaces" ...]
