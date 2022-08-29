# Rust Payments Challenge

This is a solution to the Rust Payments Challenge. This program processes
transactions from a CSV file and outputs the resulting state on stdout using the
CSV format. Errors are written to stderr.

## Running with example input file

```
cargo run -- transactions.csv
```

## Testing

A few unit tests have been written for the transaction processing function. They
should cover the most important cases. The function writing the clients' account
state to stdio is not tested as this would be a bit more complicated due to the
account lines' random order, but could definitely be done.

The functions taking input and sending output data respectively use the `std::io::Read` and a
`std::io::Write` traits to allow for easier testing and more flexibility.

```
cargo test
```

## Discussion

For this solution I assumed only deposits and withdrawals could be targeted
by a dispute. I also assumed that no transaction can be processed on a locked
account.

This solution does not use any unsafe code. Transaction processing errors are
considered non-fatal because the instructions said that the partner providing
the data may introduce some errors like adding a dispute targeting a
non-existing transaction. Errors are written to stderr.

A possible improvement could be to add custom errors that could be processed
easier by the caller, instead of just returning a string. This could allow
adding more context to an error.

This program is processing data on the fly as much as possible and does not store all
transactions in memory but only deposits and withdrawals since they are the
only ones that can be refereed to by other transactions.

If this code was to be bundled to a server and would have to process transaction
input from multiple sources at the same time then some critical resources would
have to be protected from concurrent access. For instance the clients and
transactions hash maps.

Since every transaction only targets one client, processing a transaction could
trigger a lock for its client only, allowing other transactions targeting other
clients to run concurrently. That would only work because every client is
independent to each other.
