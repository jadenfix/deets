from setuptools import setup, find_packages

with open("README.md", "r", encoding="utf-8") as fh:
    long_description = fh.read()

setup(
    name="aether-sdk",
    version="0.1.0",
    author="Aether Team",
    description="Official Python SDK for Aether Blockchain",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/aether/sdk-python",
    packages=find_packages(where="src"),
    package_dir={"": "src"},
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: Apache Software License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
    ],
    python_requires=">=3.8",
    install_requires=[
        "httpx>=0.24.0",
        "ed25519>=1.5",
        "pynacl>=1.5.0",
    ],
    extras_require={
        "dev": [
            "pytest>=7.4.0",
            "pytest-asyncio>=0.21.0",
            "black>=23.7.0",
            "mypy>=1.5.0",
        ],
    },
)

