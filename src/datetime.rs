//! ISO 8601 datetime parsing plus the range hints we support
//! (:day, :week, :month, :year, :yesterday, :today, ...).
//! Deliberately does NOT implement timewarrior's full informal date
//! grammar (DatetimeParser.cpp) -- ISO + hints only, per project scope.
