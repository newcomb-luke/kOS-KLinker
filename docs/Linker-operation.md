 
# KLinker Operation Documentation
* Version 1
* Written as of July 2021

## Contents
* [Preface](#preface)
* [About KLinker](#about-klinker)
* [Terminology](#terminology)

1. [Overview](#overview)

## Preface
This document illustrates the principles of operation of KLinker. Put simply, this document explains at a high level how KLinker works. For how to use KLinker, see the KLinker Usage Guide in this same docs folder.

KLinker was created to solve the problem of creating a compiled language for Kerbal Operating System. kOS previously did not have any compiler toolchain infrastructure in order to effectively create a compiled language. KerboScript Machine code files allow for the creation of programs not written in KerboScript, but they are not conducive to many features of modern object files such as symbols and relocation. Therefore an intermediate object file format that is closer to modern equivalents was created, KerbalObject files. But with this new file format comes the need for a [linker](https://en.wikipedia.org/wiki/Linker_(computing)).

Therefore a new linker needed to be created, and therefore KLinker was born. KLinker converts multiple KerbalObject files into one executable KSM file. KLinker can also be set to create more a shared library style KSM file, which is just loaded by executables at runtime.

### About KLinker
KLinker was created entirely from scratch in Rust, and therefore may not adhere to current best practices or best principles of operation for modern linkers such as LLVM's lld. If you have any suggestion to make about optimization or operation, please let the developers know by creating an issue on the GitHub repository.

KLinker should be fully compliant with the KO file specification. If there are any problems you find with it, be sure to report them.

KLinker is an optimizing linker. KLinker does not perform much LTO or Link-Time Optimization, but it does try to reduce binary size where possible. Currently this is done by not including dead code, or code that is never used anywhere.

### Terminology
Object file - See [Wikipedia](https://en.wikipedia.org/wiki/Object_file)

## Overview

