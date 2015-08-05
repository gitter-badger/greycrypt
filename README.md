Greycrypt is a command line program for syncing and encrypting files to a 
cloud provider storage directory.

You specify a configuration file containing directories or files that you want 
to sync, as well as the path to your local cloud storage directory.  After 
starting, Greycrypt prompts for an encryption password, and then copies 
encrypted versions of those files to the cloud storage directory.  You can 
then unpack the files on another machine by running an instance there.

Licensed under the MIT license.

### Building 

A rust nightly build is currently required.  Binary releases are not 
yet available.  If you are building on Windows, you must make sure that you 
have a working 64bit MinGW install to build the crypto package.
See https://github.com/DaGenix/rust-crypto/issues/299

The program has two build modes; Developer (Debug) and Release.  These 
versions write to different directories so that they don't interfere with 
each other.  

A release build is highly recommended for production use to reduce encryption
cpu utilization.  To build release in a unixlike environment, run:

```bash
$ sh shtool/relbuild.sh
```

On Windows, if you don't have cygwin or msys, just run the cargo command 
found in shtool/relbuild.

To build the develop version, just run "cargo build".  Or use the included 
build-and-run script
 
```bash
$ sh shtool/br.sh
```

There are various other shell utility scripts included; you can alias them
by running
```bash
$ . shtool/setup.sh
# now you can just do "br" for build and run, "relbuild", etc
```

### Tests

Run the tests with "cargo test" or using the "rt.sh" utility script.  
If any tests fail, use of this program is not recommended.

### Configuration

Configuration is done by config file.  There are two default files, 
one for release and one for develop.  The release file is named 
"config.toml", and the develop file is named "config.dbg.toml".
See "config.sample.toml" for information on how to set up a file.

### Storage

In addition to your cloud provider directory, grey crypt stores 
sync state data in "~/.greycrypt" (Mac), and in "%appdata%\GreyCrypt"
(Windows).  No unencrypted file data or other identifying information 
is stored here.

### Caveats and Limitations

* This is alpha software and it is my first Rust program.  Its also not a 
backup program; keep backups independent of greycrypt files.
* It uses filesystem polling (default 3 seconds), rather than 
events, so it is less efficient in CPU than it could be.
* It has not been tested with all cloud providers.  I have tested it with 
Google Drive and (to a lesser extent) Dropbox.
* It does not work on Linux, mainly because I have not decided how to deal
with its case-sensitive filesystem - greycrypt prefers case-insensitive
mode.  I welcome PRs that address this.
* I am not a crypto expert, so some parts of the implementation may be 
insecure.  I welcome an audit or suggests from a trained crypto engineer.
* GreyCrypt can remove files; if you remove a file in a synced directory,
other systems that are syncing that file will remove it as well.
GreyCrypt uses the system trash/recycling bin, but its important to 
remember that this means the unencrypted file will sit in that bin 
for however long it takes the OS to remove it.  
* The command line options for conflict resolution are rough and 
not super useful.