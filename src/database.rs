//! Aggregates all datafiles under data/, and assigns ephemeral interval
//! IDs by counting backward from the most recent interval (id=1 = most
//! recent / the open interval if any). IDs are NEVER stored on disk --
//! this must be recomputed on every invocation. Mirrors data.cpp::getTracked.
