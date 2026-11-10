//! Overlap detection and the :adjust auto-resolution logic.
//! Must match validate.cpp::autoAdjust exactly -- this is the code path
//! responsible for the "You cannot overlap intervals" error and for
//! safely trimming/splitting neighboring intervals when :adjust is given.
