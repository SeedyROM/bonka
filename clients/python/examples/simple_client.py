from bonka.client import BonkaClient


def main():
    # Create a client
    client = BonkaClient(host="localhost", port=8379)

    try:
        # Connect to the server
        client.connect()
        print("Connected to Bonka server")

        # Set some values
        client.set("name", "Alice")
        client.set("age", 30)
        client.set("active", True)
        print("Set three values")

        # Get values
        name = client.get("name")
        age = client.get("age")
        active = client.get("active")
        print(f"Retrieved values: name={name}, age={age}, active={active}")

        # List keys
        keys = client.list()
        print(f"All keys: {keys}")

        # Delete a key
        deleted = client.delete("age")
        print(f"Deleted 'age': {deleted}")

        # List keys again
        keys = client.list()
        print(f"Keys after delete: {keys}")

    except Exception as e:
        print(f"Error: {e}")
    finally:
        # Clean up
        if client.connected:
            client.disconnect()
            print("Disconnected from server")


if __name__ == "__main__":
    main()
