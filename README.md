Greycrypt is a command line program for syncing and encrypting files to a 
cloud provider storage directory.

You specify a configuration file containing directories or files that you want 
to sync, as well as the path to your local cloud storage directory.  After 
starting, Greycrypt prompts for an encryption password, and then copies 
encrypted versions of those files to the cloud storage directory.  You can 
then unpack the files on another machine by running an instance of your 
cloud sync program and GreyCrypt.

Licensed under the MIT license.

### Source Hosting

[![Join the chat at https://gitter.im/jmquigs/greycrypt](https://badges.gitter.im/Join%20Chat.svg)](https://gitter.im/jmquigs/greycrypt?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge&utm_content=badge)

This repository is dual-hosted; I keep "master" in sync on both, other 
branches may diverge.

* https://bitbucket.org/jmquigs/greycrypt
* https://github.com/jmquigs/greycrypt

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
sync state data in "~/.greycrypt" (Mac), or in "%appdata%\GreyCrypt"
(Windows).  No unencrypted file data or other identifying information 
is stored here.

### Application Lock Files

Some applications write temporary lock files to storage when a 
document is open for editing; these are to prevent multiple
instances of the application from opening the same file.  
There are two forms of this: the first uses exclusive write
locks so that no other process can write to the lock file; 
the second allows writes, and just puts information into the 
lock file indicating who is editing it.

GreyCrypt doesn't have any special knowledge of these files and
will attempt to sync them.  However, the first form 
(exclusive lock) will cause sync failures, because it won't be 
able to write the output file.  If you use a program that 
produces these lock files with GreyCrypt, it is recommended that
you add the lock file to the ignore list (TODO: doc how).  You'll
then need to be careful that you don't overwrite the file from
two different machines with different data. 

The second form of lock is handled adequately by GreyCrypt.  
OpenOffice/LibreOffice is an example that uses this kind of lock file.

### Resolving conflicts

Occasionally a sync will produce conflicts; usually this is when 
a file with the same name and keyword mapping, but different contents,
is synced from two different computers.  

Right now you must resolve 
these manually.  Run greycrypt with the "-x" option, which will show
you a list of the conflicting files; remove one of the conflicting sync
files to remove the conflict.  You may need to remove or rename the
source local file on one machine to keep the conflict from recurring.

### Caveats and Limitations

* This is alpha software and it is my first Rust program.  Its also not a 
backup program; keep backups independent of greycrypt files.  However,
I do use it with my own files.
* It uses filesystem polling (default 3 seconds), rather than 
events, so it is less efficient in CPU than it could be.
* It has not been tested with all cloud providers.  I have tested it with 
Google Drive and (to a lesser extent) Dropbox.
* It does not work on Linux, mainly because I have not decided how to deal
with its case-sensitive filesystem - GreyCrypt prefers case-insensitive
mode.  I welcome PRs that address this.
* I am not a crypto expert, so some parts of the implementation may be 
insecure.  I welcome an audit or suggestions from a trained crypto engineer.
* GreyCrypt can remove files; if you remove a file in a synced directory,
other systems that are syncing that file will remove it as well.
GreyCrypt uses the system trash/recycling bin, but its important to 
remember that this means the unencrypted file will sit in that bin 
for however long it takes the OS to remove it.  
* The command line options for conflict resolution are rough and 
not super useful.