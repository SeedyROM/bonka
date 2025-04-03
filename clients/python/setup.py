from setuptools import setup, find_packages

setup(
    name="bonka-client",
    version="0.1.0",
    packages=find_packages(),
    install_requires=[
        "protobuf>=4.21.0",
    ],
    python_requires=">=3.7",
)
