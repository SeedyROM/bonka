from setuptools import setup, find_packages

setup(
    name="bonka-client",
    version="0.1.0",
    packages=find_packages(),
    license="MIT or Apache-2.0",
    author="SeedyROM (Zack Kollar)",
    install_requires=[
        "protobuf>=4.21.0",
    ],
    python_requires=">=3.7",
)
