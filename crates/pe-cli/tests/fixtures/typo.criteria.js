// A misspelled category. It parses fine as a query, so nothing can refuse it --
// but it matches no cards, which would silently produce a confident 0%.
criterion("misspelled category", (t) => t(0).count('cat:"Rmap"') >= 1, { atLeast: 0.3 });
