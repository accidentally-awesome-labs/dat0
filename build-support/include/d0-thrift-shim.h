// Build shim: give duckdb's vendored Thrift enum iterator an `operator==`.
//
// # The failure
//
// `libduckdb-sys`'s bundled Parquet extension builds tables like
//
//     const std::map<int, const char*> _Type_VALUES_TO_NAMES(
//         apache::thrift::TEnumIterator(8, _kTypeValues, _kTypeNames),
//         apache::thrift::TEnumIterator(-1, nullptr, nullptr));
//
// `TEnumIterator` (third_party/thrift/thrift/Thrift.h) declares `operator!=`
// and **no** `operator==`. That was sufficient for older libc++, whose
// `__tree::__insert_range_unique` compared iterators with `!=`. Current libc++
// — Apple clang 21 / macOS SDK 26 — compares with `==`, so the build dies with
//
//     error: invalid operands to binary expression
//            ('duckdb_apache::thrift::TEnumIterator' and '...')
//
// The bug is upstream and is still present in duckdb 1.4.5 (checked), so
// waiting for a patch release is not a plan. It is invisible on any machine
// with a warm `target/` or an older SDK, which is exactly why it wants a fix in
// the tree rather than a note in someone's shell history.
//
// # The fix
//
// Declare the missing operator ourselves, `-include`d ahead of every C++
// translation unit in the build (see `.cargo/config.toml`). Three details make
// it safe:
//
//   * It is a **template**, so its body is only instantiated where it is
//     actually used — by which point `TEnumIterator` is a complete type. A
//     plain inline function could not be defined here, because at this point
//     the class is only forward-declared.
//   * `enable_if` pins it to exactly `TEnumIterator`, so it cannot become a
//     catch-all `operator==` for unrelated types in these namespaces.
//   * The body calls `operator!=` **explicitly as a member**, never as an
//     operator. Under C++20 a plain `a != b` here could resolve to the
//     rewritten `!(a == b)` and recurse into this very function.
//
// `const_cast` because upstream's `operator!=` is a non-const member.
//
// Delete this file, and both env vars in the two `.cargo/config.toml`s, once
// duckdb ships a `TEnumIterator::operator==`.

#pragma once

#include <type_traits>

// duckdb renames the vendored namespace to avoid colliding with a system
// Thrift; the alias `apache::thrift` used at the call sites resolves here.
namespace duckdb_apache {
namespace thrift {

class TEnumIterator;

template <class T,
          class = typename std::enable_if<std::is_same<T, TEnumIterator>::value>::type>
inline bool operator==(const T& lhs, const T& rhs) {
  return !const_cast<T&>(lhs).operator!=(rhs);
}

}  // namespace thrift
}  // namespace duckdb_apache
