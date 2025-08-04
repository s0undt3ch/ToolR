Here's some basic usage examples to start off of.


## Simple Command

The most basic command is a function with a context parameter:

```python title="tools/hello.py"
--8<-- "docs/examples/files/hello.py"
```

Run with:
```bash
toolr greeting hello --name Alice
# Output: Hello, Alice!
```

## Command with Multiple Arguments

```python title="tools/calculator.py"
--8<-- "docs/examples/files/calculator.py"
```

Run with:
```bash
toolr math add 5 3
# Output: 5 + 3 = 8
```

## Boolean Flags

```python title="tools/example.py"
--8<-- "docs/examples/files/example.py"
```

Run with:
```bash
toolr example process --verbose --dry-run
# Output: Verbose mode enabled
#         Dry run mode - no changes will be made
```

## List Arguments

```python title="tools/files.py"
--8<-- "docs/examples/files/files.py"
```

Run with:
```bash
toolr files process-files file1.txt file2.txt file3.txt
# Output: Processing file1.txt...
#         Processing file2.txt...
#         Processing file3.txt...
```

The above command could also be defined like:

```python title="tools/files.py"
--8<-- "docs/examples/files/files-star-args.py"
```

## Using the Context

The `ctx` parameter provides access to useful utilities:

```python title="tools/system.py"
--8<-- "docs/examples/files/system.py"
```

Run with:
```bash
toolr system info
```
