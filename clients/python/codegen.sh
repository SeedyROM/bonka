#!/bin/bash

python -m grpc_tools.protoc -I=../../protocol --python_out=bonka/proto --grpc_python_out=bonka/proto bonka.proto