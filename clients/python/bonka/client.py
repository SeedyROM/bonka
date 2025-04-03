import socket
import struct
import time
from typing import Optional, Union, List
from bonka.proto.bonka_pb2 import Request, Response, CommandType, ResultType, Value


class BonkaClient:
    def __init__(self, host: str = "localhost", port: int = 8379):
        self.host = host
        self.port = port
        self.socket = None
        self.connected = False
        self.request_id = 0

    def connect(self) -> None:
        """Connect to the Bonka server."""
        # Try with explicit IPv4
        try:
            self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            # Add socket options that might help
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            # Set a timeout so we don't hang forever
            self.socket.settimeout(5)
            # Connect to explicit IP instead of hostname
            self.socket.connect((self.host, self.port))
            self.connected = True
            print(f"Successfully connected to {self.host}:{self.port}")
        except Exception as e:
            # If IPv4 fails, try IPv6
            print(f"IPv4 connection failed: {e}")
            try:
                self.socket = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
                self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                self.socket.settimeout(5)
                self.socket.connect((self.host, self.port))
                self.connected = True
                print(f"Successfully connected using IPv6 to {self.host}:{self.port}")
            except Exception as e6:
                raise Exception(f"Failed to connect: IPv4 error: {e}, IPv6 error: {e6}")

    def disconnect(self) -> None:
        """Disconnect from the server by sending EXIT command."""
        if self.connected:
            self._send_exit_command()
            self.socket.close()
            self.connected = False

    def _send_exit_command(self) -> None:
        """Send an EXIT command to the server."""
        request = Request(
            id=self._next_id(),
            timestamp=int(time.time()),
            command_type=CommandType.COMMAND_EXIT,
        )
        self._send_request(request)

    def _next_id(self) -> int:
        """Generate the next request ID."""
        self.request_id += 1
        return self.request_id

    def get(self, key: str) -> Optional[Union[str, bytes, int, float, bool]]:
        """Get a value from the server."""
        request = Request(
            id=self._next_id(),
            timestamp=int(time.time()),
            command_type=CommandType.COMMAND_GET,
            key=key,
        )

        response = self._send_request(request)

        if response.result_type == ResultType.RESULT_VALUE:
            if not hasattr(response, "value") or not response.value:
                return None

            # Extract the appropriate value based on which field is set
            if response.value.HasField("string_value"):
                return response.value.string_value
            elif response.value.HasField("bytes_value"):
                return response.value.bytes_value
            elif response.value.HasField("int_value"):
                return response.value.int_value
            elif response.value.HasField("uint_value"):
                return response.value.uint_value
            elif response.value.HasField("float_value"):
                return response.value.float_value
            elif response.value.HasField("bool_value"):
                return response.value.bool_value
            return None
        elif response.result_type == ResultType.RESULT_ERROR:
            raise Exception(f"Error from server: {response.error}")
        else:
            return None

    def set(self, key: str, value: Union[str, bytes, int, float, bool]) -> None:
        """Set a key-value pair on the server."""
        # Create the protobuf Value object
        proto_value = Value()

        # Set the appropriate field based on the Python type
        if isinstance(value, str):
            proto_value.string_value = value
        elif isinstance(value, bytes):
            proto_value.bytes_value = value
        elif isinstance(value, int):
            if value < 0:
                proto_value.int_value = value
            else:
                proto_value.uint_value = value
        elif isinstance(value, float):
            proto_value.float_value = value
        elif isinstance(value, bool):
            proto_value.bool_value = value
        else:
            raise TypeError(f"Unsupported value type: {type(value)}")

        # Create the request
        request = Request(
            id=self._next_id(),
            timestamp=int(time.time()),
            command_type=CommandType.COMMAND_SET,
            key=key,
            value=proto_value,
        )

        response = self._send_request(request)

        if response.result_type == ResultType.RESULT_ERROR:
            raise Exception(f"Error from server: {response.error}")

    def delete(self, key: str) -> bool:
        """Delete a key from the server."""
        request = Request(
            id=self._next_id(),
            timestamp=int(time.time()),
            command_type=CommandType.COMMAND_DELETE,
            key=key,
        )

        response = self._send_request(request)

        if response.result_type == ResultType.RESULT_SUCCESS:
            return True
        elif response.result_type == ResultType.RESULT_ERROR:
            return False
        else:
            return False

    def list(self) -> List[str]:
        """List all keys on the server."""
        request = Request(
            id=self._next_id(),
            timestamp=int(time.time()),
            command_type=CommandType.COMMAND_LIST,
        )

        response = self._send_request(request)

        if response.result_type == ResultType.RESULT_KEYS:
            return list(response.keys)
        elif response.result_type == ResultType.RESULT_ERROR:
            raise Exception(f"Error from server: {response.error}")
        else:
            return []

    def _send_request(self, request: Request) -> Response:
        """Send a request to the server and return the response."""
        if not self.connected:
            raise Exception("Not connected to server")

        # Serialize the request
        data = request.SerializeToString()

        # Send length-prefixed message
        self.socket.sendall(struct.pack(">I", len(data)))
        self.socket.sendall(data)

        # Receive response
        length_bytes = self.socket.recv(4)
        if not length_bytes:
            raise Exception("Connection closed by server")

        length = struct.unpack(">I", length_bytes)[0]
        response_bytes = b""

        # Ensure we read the complete message
        while len(response_bytes) < length:
            chunk = self.socket.recv(length - len(response_bytes))
            if not chunk:
                raise Exception("Connection closed by server")
            response_bytes += chunk

        # Parse response
        response = Response()
        response.ParseFromString(response_bytes)

        return response
