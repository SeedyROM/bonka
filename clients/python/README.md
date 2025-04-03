```
d8b                         d8b                   
 ?88                         ?88                 
  88b                         88b                
  888888b  d8888b   88bd88b   888  d88' d888b8b  
  88P `?8bd8P' ?88  88P' ?8b  888bd8P' d8P' ?88  
 d88,  d8888b  d88 d88   88P d88888b   88b  ,88b 
d888888P'`?8888P'd88'   88bd88' `?88b,`?88P'`88b
```

[![GitHub](https://img.shields.io/badge/github-bonka-8da0cb?logo=GitHub)](https://github.com/SeedyROM/bonka/blob/master/clients/python)
[![License: MIT](https://img.shields.io/badge/license-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![License](https://img.shields.io/badge/license-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

A Python client for [Bonka](https://github.com/SeedyROM/bonka), a lightweight key-value store.

## Installation

```bash
# Install from PyPI (coming soon)
pip install bonka-client

# Or install directly from the repository
pip install git+https://github.com/SeedyROM/bonka.git#subdirectory=clients/python
```

## Quick Start

```python
from bonka.client import BonkaClient

# Create a client
client = BonkaClient(host="localhost", port=8379)

# Connect to the server
client.connect()

# Set values of different types
client.set("name", "Alice")
client.set("age", 30)
client.set("balance", 155.75)
client.set("active", True)
client.set("data", b"\x01\x02\x03\x04")

# Get values
name = client.get("name")  # Returns "Alice"
age = client.get("age")    # Returns 30

# List all keys
keys = client.list()       # Returns ["name", "age", "balance", "active", "data"]

# Delete a key
deleted = client.delete("age")  # Returns True if successful

# Clean up when done
client.disconnect()
```

## API Reference

### `BonkaClient`

The main client class for interacting with a Bonka server.

#### `__init__(host="localhost", port=8379)`

Create a new Bonka client.

- `host` (str): The hostname or IP address of the Bonka server.
- `port` (int): The port number of the Bonka server.

#### `connect()`

Connect to the Bonka server. Must be called before any other operations.

Raises an exception if the connection fails.

#### `disconnect()`

Gracefully close the connection to the server. 

#### `get(key)`

Get a value from the server.

- `key` (str): The key to retrieve.
- Returns: The value associated with the key, or `None` if the key doesn't exist.
  - The return type depends on what was stored (str, bytes, int, float, bool).

#### `set(key, value)`

Set a key-value pair on the server.

- `key` (str): The key to set.
- `value`: The value to set. Can be a string, bytes, integer, float, or boolean.
- Raises an exception if the operation fails.

#### `delete(key)`

Delete a key from the server.

- `key` (str): The key to delete.
- Returns: `True` if the key was deleted, `False` if the key didn't exist.

#### `list()`

List all keys on the server.

- Returns: A list of strings representing all keys.

## Supported Value Types

Bonka supports the following value types:

- Strings (`str`)
- Bytes (`bytes`)
- Integers (`int`)
- Unsigned Integers (`int`, positive values)
- Floating-point numbers (`float`)
- Booleans (`bool`)
- Null (represented as `None` in Python)

## Error Handling

All client methods that communicate with the server can raise exceptions. It's recommended to wrap client operations in try-except blocks:

```python
try:
    client.connect()
    client.set("key", "value")
    value = client.get("key")
except Exception as e:
    print(f"Error: {e}")
finally:
    if client.connected:
        client.disconnect()
```

## Development

### Requirements

- Python 3.7+
- protobuf >= 4.21.0

### Running Tests

```bash
# Coming soon
```

## License

This project is licensed under the MIT License or Apache License 2.0, at your option - see the [LICENSE](../../LICENSE) file for details.