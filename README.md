# Kerbal Linker (kld)

[<img src="https://img.shields.io/badge/github-newcomb--luke%2FkOS--KLinker-8da0cb?style=for-the-badge&logo=github&labelColor=555555" alt="github" height="24">](https://github.com/newcomb-luke/kOS-KLinker)
[<img src="https://img.shields.io/crates/v/klinker?color=fc8d62&logo=rust&style=for-the-badge" alt="github" height="24">](https://crates.io/crates/klinker)
[<img alt="License" src="https://img.shields.io/github/license/newcomb-luke/kOS-KLinker?style=for-the-badge" height="24">]()

[<img alt="GitHub Workflow Status" src="https://img.shields.io/github/actions/workflow/status/newcomb-luke/kOS-KLinker/main.yml?style=for-the-badge" height="24">]()
[<img alt="Libraries.io dependency status for GitHub repo" src="https://img.shields.io/librariesio/github/newcomb-luke/kOS-KLinker?style=for-the-badge" height="24">](https://deps.rs/repo/github/newcomb-luke/kOS-KLinker)
[<img alt="Crates.io Downloads" src="https://img.shields.io/crates/d/klinker?style=for-the-badge" height="24">]()

The Kerbal Linker, or KLinker is a completely custom built linker designed to support development of a compiler toolchain for Kerbal Operating System. KLinker links KerbalObject files into KerboScript Machine Code files which can be run inside of kOS.

To be able to inspect the files that this program consumes and outputs, the tool [kDump](https://github.com/newcomb-luke/KDump) was created.

This linker will successfully create both executables and shared libraries if supplied compliant KerbalObject files that follow the KerbalObject file format [specification](https://github.com/newcomb-luke/kOS-KLinker/blob/main/docs/KO-file-format.md). These KerbalObject files can be created by any assembler or compiler built to emit them, such as the [Kerbal Assembler](https://github.com/newcomb-luke/kOS-KASM). 

## Features

* Symbol relocation
* Local symbol support
* Link-time file size optimization

## Installation

The Kerbal Linker can either be installed via [cargo](https://github.com/rust-lang/cargo) through [crates.io](https://crates.io/), or as a standalone binary.

#### Windows

- Download the .msi file from Releases on the right
- Run the installer
- **kld** should now be added to your PATH and available from any CMD or Powershell window

#### Arch Linux

- Download the PKGBUILD from Releases on the right

- Copy it to a temporary folder

- Run `makepkg -si` to install **klinker** and all of its dependencies.

- **kld** should now be added to your PATH and available from any terminal

#### Standalone Executables

- Download and extract the .zip file from Releases on the right
- Place the executable in the desired location
- Run the executable through the terminal, Powershell on Windows or the default terminal on Mac OS or Linux.

#### Cargo

```
cargo install klinker
```

**kld** should then be added to your shell's PATH, and can be run from any terminal

## Usage

The Kerbal Linker can be invoked after installation as **kld**

Help can be accessed from the program itself by running:

```
kld --help
```

The basic format for kld arguments is:

```
kld [FLAGS] [OPTIONS] <INPUT>... -o <OUTPUT PATH>
```

When running `kld`, the linker cannot infer the output file name, so one must always be specified. This is accomplished by passing the **-o** flag to kld:

```
kld -o myprogram.ksm
```

If the output path does not end in .ksm, kld will attempt to add it.

kld is able to take more than one file as input at a time, and multiple input files are input as paths separated by spaces:

```
kld librocket.ko mathlib.ko main.ko -o launch.ksm
```

The **-s** flag can be specified to put the linker into shared library mode. This mode requires only the _init function to be present.

```
kld libdock.ko librendezv.ko -o docking.ksm
```

This file cannot be run directly, and instead should be loaded from another program.

kld also allows the user to specify the entry point of the program, or the function that the program starts running from. By default this is the _start function. This can be changed by using the **-e** flag:

```
kld -e __main__ main.ko -o program.ksm
```

This will make the linker search for a function with the name "\_\_main\_\_" and then create the KSM file so that that code is what is run when the program starts up.

## Notes

The Kerbal Linker currently uses link-time file size optimization. This feature may be able to be disabled by a command line flag if its operation is deemed to be unwanted in specific cases. Currently this works by finding out which functions inside all of the KerbalObject files are actually referenced from code that could have the possibility of being run. If a function is not referenced anywhere that is also referenced, then that function is not included in the final KSM file. This means that for code such as a program language's standard library that is almost never all completely used, file sizes will not be rediculously large.

This contrasts with how KerboScript works inside kOS, because KerboScript code is all loaded at runtime through running other scripts, all of the code must be present, which means that any code that is compiled and turned into KerbalObject files can be way smaller than equivalent KerboScript libraries.
