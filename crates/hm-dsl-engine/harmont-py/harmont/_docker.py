"""Docker toolchain and CI-pipeline DSL.

Public surface is the module-level ``docker`` namespace:

    hm.docker.toolchain(...)  -> DockerToolchain  (install-only)
    hm.docker.project(...)    -> DockerProject    (full CI DAG)

Chain: scratch -> apt-base (curl, ca-certificates) ->
convenience script install -> action leaves.
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Self, Optional
from urllib.parse import urlparse
from enum import Enum

from ._toolchain import advance_install, make_install_chain
from .cache import CacheForever, CacheOnChange


if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy

APT_PACKAGES = ("curl", "ca-certificates")
_VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+$")  # e.g. 3.3.8
_MIN_DOCKER_VERSION = (19, 3) # Docker minimal version, don't forget to add -ce

_SHORT_FLAGS = {"file": "-f", "tag": "-t", "output": "-o", "quiet": "-q", "debug": "-D"}

def _parse_version(version: str) -> tuple[int, ...]:
    return tuple(int(p) for p in version.split("."))

def _docker_install_cmd_using_convenience_script(channel: str, version: str = "20.10") -> str:
    """This uses the convenience script as outline here 
    
    https://docs.docker.com/engine/install/ubuntu/#install-using-the-convenience-script

    curl -fsSL https://get.docker.com -o get-docker.sh
    sudo sh ./get-docker.sh --dry-run
    """

    base  = "curl -fsSL https://get.docker.com -o get-docker.sh && sudo sh get-docker.sh"
    parts = []

    if channel :
        if channel not in ("stable", "test"):
            msg = f"hm.docker: channel must be 'stable' or 'test', got {channel!r}"
            raise ValueError(msg) 
        parts.append(f"--channel {channel}")

    if version:
        parts.append(f"--version {version}")

    if parts:
        return f"{base} {' '.join(parts)}"
    
    return base

class Context(Enum):
    """Build context type for ``docker build``.

    LOCAL  — local directory, tarball, or stdin with path
    REMOTE — remote URL or git repository
    EMPTY  — stdin only (``docker build -``)
    """
    LOCAL = "local"
    REMOTE = "remote"
    EMPTY = "empty"

@dataclass(frozen=True)
class DockerOpts:
    """Docker build Options when you ran `docker build --help`
    """
    opts: tuple[tuple[str, Any], ...] = ()

    @staticmethod
    def from_kwargs(**kwargs: Any) -> DockerOpts:
        return DockerOpts(opts=tuple(kwargs.items()))

@dataclass(frozen=True)
class Repeated:
    repeated: tuple[str, ...] = ()

def docker_flags(docker_opts: DockerOpts) -> str:
    """Assemble the docker options command. DockerOpts 'options' variable contains
    any flag that is of type --name stringArray
    """
    # probably validate later
    # use cargo_flags as reference 
    toks: list[str] = []

    for name, value in docker_opts.opts:
        flag = _SHORT_FLAGS.get(name) or "--" + name.replace("_", "-")

        if isinstance(value, bool):
            if value:
                toks.append(flag)
        elif isinstance(value, Repeated):
            for v in value.repeated:
                toks.append(f"{flag} {shlex.quote(v)}")
        elif isinstance(value, tuple):
            toks.append(f"{flag} {','.join(shlex.quote(v) for v in value)}")
        elif isinstance(value, str) and value:
            toks.append(f"{flag} {shlex.quote(value)}")
            
    return (" ".join(toks)) if toks else ""

def _build_empty_context(
        dockerfile: bool, 
        contents: list[str], 
        options: Optional[DockerOpts] = None, 
        path: str = "."
    ) -> str:   
    """This returns the pattern below, a) either with the name DockerFile or b) the 
        contents of the DockerFile passed as heredoc

        a) docker build - < Dockerfile
        
        b)  docker build - <<EOF
            FROM ubuntu:22.04
            RUN apt-get update && apt-get install -y curl
            ENV MY_VAR=hello
            WORKDIR /app
            CMD ["bash"]
            EOF
    """
    if options:
        flags = docker_flags(options) 

        if dockerfile:
            return f"docker build {flags} - < Dockerfile"

        if contents:
            contents = "\n".join(contents)
            return (
                f"docker build {flags} - <<EOF\n"
                f"{contents}"
                "EOF"
            )

        if path:
            return f"docker build {flags} {path}"
        
    else:
        if dockerfile:
            return f"docker build - < Dockerfile"
            
        if contents:
            contents = "\n".join(contents)
            return (
                f"docker build  - <<EOF\n"
                f"{contents}"
                "EOF"
            )

        if path:
            return f"docker build {path}"

def _build_remote_context(
    contents: list[str],
    url: str,
    options: Optional[DockerOpts] = None
) -> str:
    """context options are remote, empty This returns the pattern below
    remote contexts: https://docs.docker.com/build/concepts/context/#remote-context
    
    a) remote url docker build -f- <URL>
    b) remote context with Dcokerfile from stdin

    docker build -t myimage:latest -f- https://github.com/docker-library/hello-world.git <<EOF
    FROM busybox
    COPY hello.c ./
    EOF

    c) remote tarball - docker build http://server/context.tar.gz
    """

    result = urlparse(url)
    valid_url = all([result.scheme, result.netloc])

    if not valid_url:
        msg = f"the url {valid_url} is not a valid url"
        raise ValueError(msg)

    if options:
        flags = docker_flags(options)

        if contents:
            contents = "\n".join(contents)
            return (
                f"docker build {flags} -f- {url} <<EOF\n"
                f"{contents}"
                "EOF"
            )

        return f"docker build {flags} -f- {url}"  
    else:
        if contents:
            contents = "\n".join(contents)
            return (
                f"docker build -f- {url} <<EOF\n"
                f"{contents}"
                "EOF"
            )

        return f"docker build -f- {url}"
        
def _build_local_context(
    contents: list[str], 
    options: Optional[DockerOpts] = None,
    tarball: Optional[str] = None,
    path: str = "."
) -> str:
    """context options are local, empty This returns the pattern below
    local contexts: https://docs.docker.com/build/concepts/context/#local-context
    
    a) local directories - docker build <options> .
    b) local context with Dockerfile from stdin

    docker build -f- <PATH> <<EOF
    FROM busybox
    COPY somefile.txt ./
    RUN cat /somefile.txt
    EOF

    c) docker build <options> - < foo.tar.gz
    """

    if options:
        flags = docker_flags(options)

        if contents:
            contents = "\n".join(contents)
            return (
                f"docker build {flags} -f- {path} <<EOF\n"
                f"{contents}"
                "EOF"
            )

        if tarball:
            extension = tarball.split('.')[-1]

            if extension not in ('gz', 'tgz'):
                msg = (
                    "Please ensure a tarball with file extension format is passed. \n" \
                    f" the tarball {tarball} received doesn't match this"
                )
                raise ValueError(msg)
            
            return f"docker build {flags} - < {tarball}"
            
        return f"docker build {flags} ."
        
    else:
        if contents:
            contents = "\n".join(contents)
            return (
                f"docker build -f- {path} <<EOF\n"
                f"{contents}"
                "EOF"
            )
        
        if tarball:
            extension = tarball.split('.')[-1]

            if extension not in ('gz', 'tgz'):
                msg = (
                    "Please ensure a tarball with file extension format is passed. \n" \
                    f" the tarball {tarball} received doesn't match this"
                )
                raise ValueError(msg)
            
            return f"docker build - < {tarball}"

        return f"docker build ."
        
def _run_command(
    image: str,
    command_with_args: tuple[str, ...],
    options: Optional[DockerOpts] = None
) -> str:
    """ Docker run command which takes \n
    
    a. image \n
    b. command \n
    c. args passed to the command \n

    Usage:  docker run [OPTIONS] IMAGE [COMMAND] [ARG...]
    """
    if not command_with_args:
        msg = f"command with args is empty {command_with_args}"
        raise ValueError(msg)
    
    command = command_with_args[0]
    args    = command_with_args[1:]

    if options:
        flags = docker_flags(options) 

        if command:
            args_str = " ".join(args)
            if args:
                return f"docker run {flags} {image} {command} {args_str}"
            else:
                return f"docker run {flags} {image} {command}"
        else:
            msg = f"command with args is empty {command}"
            raise ValueError(msg)
    else:
        if command:
            args_str = " ".join(args)
            if args:
                return f"docker run {image} {command} {args_str}"
            else:
                return f"docker run {image} {command}"
        else:
            msg = f"command with args is empty {command}"
            raise ValueError(msg)

def _run_common_command(
    command_with_args: tuple[str, ...] = (),
    options: Optional[DockerOpts] = None
) -> str:
    """
    Run any listed under Common Commands when you run `docker help` command

    Usage:  docker [OPTIONS] COMMAND

    A self-sufficient runtime for containers

    Common Commands:
    run         Create and run a new container from an image
    exec        Execute a command in a running container
    """

    if not command_with_args: # if () raise this
        msg = f"command with args is empty {command_with_args}"
        raise ValueError(msg)

    if not command_with_args[0]: # if ('', ('arg', 'arg2')) throw this
        msg = (
            "hm.docker: docker command is empty \n"
            "please pass a common command like run, ps, version etc"
        )
        raise ValueError(msg)

    if len(command_with_args) == 1:
        return f"docker {command_with_args[0]}"

    command = command_with_args[0]
    args    = command_with_args[1:]
    args_str = " ".join(args)

    if command:
        if options:
            flags = docker_flags(options) 
            
            return f"docker {flags} {command} {args_str}"
            
        else:
            return f"docker {command} {args_str}"

    else:
        msg = (
            "hm.docker: docker command is empty \n"
            "please pass a common command like run, ps, version etc"
        )
        raise ValueError(msg)

@dataclass(frozen=True)
class DockerToolchain:
    """Docker toolchain install chain — constructed via ``hm.docker.toolchain()``.

    Holds the docker install step. Action methods (``build``, ``run``)
    attach leaves to ``installed``. Use ``hm.docker.project()`` when you
    want a warmup step shared across actions.
    """

    path: str
    installed: Step

    def setup(
        self,
        cmd: str,
        *,
        cwd: str | None = None,
        label: str | None = None,
        cache: CachePolicy | None = None,
        env: dict[str, str] | None =  None
    ) -> Self:
        """Append a post-install command and return an advanced toolchain; chainable.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.docker.toolchain(path=".").setup("")
        """
        return advance_install(self, cmd, cwd=cwd, label=label, cache=cache, env=env)

    def _emit(self, docker_cmd: str, default_label: str, **kw) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.installed.sh(docker_cmd, **kw)

    def build(self, *, context: Context = Context.LOCAL, **kw: Any) -> Step:
        """Build a Docker image from the given context.

        Args:
            context: LOCAL, REMOTE, or EMPTY — determines which kwargs apply.
            path: build context path (LOCAL only, default ".").
            tarball: path to .tar.gz or .tgz (LOCAL only).
            url: remote URL or git repo (REMOTE only).
            dockerfile: use Dockerfile from stdin (EMPTY only).
            contents: Dockerfile lines as heredoc (any context).
            options: DockerOpts for extra flags.
        """
        
        contents = kw.pop("contents", None)
        options  = DockerOpts.from_kwargs(**kw.pop("options", {}))

        match context:
            case Context.LOCAL:
                tarball = kw.pop("tarball", None)
                path = kw.pop("path", ".")

                cmd = _build_local_context(
                    contents=contents, 
                    options=options, 
                    tarball=tarball, 
                    path=path
                )

            case Context.REMOTE:
                url = kw.pop("url", None)
                cmd = _build_remote_context(contents=contents, url=url, options=options)

            case Context.EMPTY:
                use_dockerfile: bool = kw.pop('dockerfile', False)

                if use_dockerfile:
                    cmd = _build_empty_context(
                        True, 
                        contents=contents, 
                        options=options)
                else:
                    cmd = _build_empty_context(
                        False, 
                        contents=contents, 
                        options=options)

        label = f":docker: {self.path} docker build"
        return self._emit(cmd, label, **kw)

    def run(
        self,
        image: str,
        command_with_args: tuple[str, ...],
        options: Optional[DockerOpts] = None     
    ) -> Step:
        """Run a container from an image (``docker run``).

        Args:
            image: image name and optional tag.
            command_with_args: command and args to pass to the container.
            options: DockerOpts for extra flags.
        """
        cmd = _run_command(
            image=image,
            command_with_args=command_with_args,
            options=options
        )
        return self._emit(cmd, f":docker: {self.path} docker run", **{})

@dataclass(frozen=True)
class DockerProject:
    """High-level Docker CI DAG — constructed via ``hm.docker.project()``.

    Action methods (``build``, ``run``, ``version``) attach leaves to the
    shared warmup step so the base image pull is reused. ``ci()`` returns
    the standard DAG in one call.
    """
    path: str
    toolchain: DockerToolchain
    warmup: Step

    def _emit(self, docker_cmd: str, default_label: str, **kw) -> Step:
        if kw.get("label") is None:
            kw["label"] = default_label
        return self.warmup.sh(docker_cmd, **kw)

    def build(self, *, context: Context = Context.LOCAL, **kw: Any) -> Step:
        """
        "Build the image from the given context and return its step.

        ``context`` selects the docker build context type and determines which
        options apply:

        """
        contents = kw.pop("contents", None)
        options  = DockerOpts.from_kwargs(**kw.pop("options", {}))

        match context:
            case Context.LOCAL:
                tarball = kw.pop("tarball", None)
                path = kw.pop("path", None)
                
                cmd = _build_local_context(
                    contents=contents, 
                    options=options, 
                    tarball=tarball, 
                    path=path
                )
                
            case Context.REMOTE:
                url = kw.pop("url", None)

                if url is None:
                    msg = (
                        f"Please pass a url for remote context for 'docker build' \n"
                        "See https://docs.docker.com/build/concepts/context/#remote-context"
                    )
                    raise ValueError(msg)
                
                cmd = _build_remote_context(contents=contents, url=url, options=options)

            case Context.EMPTY:
                use_dockerfile = kw.pop('dockerfile', None)
                path = kw.pop("path", None)

                if use_dockerfile:
                    cmd = _build_empty_context(
                        True, 
                        contents=contents, 
                        options=options, 
                        path=path)
                else:
                    cmd = _build_empty_context(False, contents=contents, options=options)
        return self._emit(cmd, f":docker: {self.path} docker build", **kw)

    def run(
        self,
        command_with_args: tuple[str, ...],
        image: str = "hello-world",
        options: Optional[DockerOpts] = None,
        **kw  
    ) -> Step:
        """
        Docker run command for taking in an image, options and commands to pass to the running container
        if no image is passed in, the docker image ``hello-world`` is pulled
        """
        cmd = _run_command(
            image=image,
            command_with_args=command_with_args,
            options=options
        )
        return self._emit(cmd, f":docker: {self.path} docker run", **kw)

    def version(self) -> Step:
        """Print Docker version info (``docker version``)."""
        version = ('version',)
        cmd = _run_common_command(version)
        return self._emit(cmd, f":docker: {self.path} docker version", **{})

    def ci(self) -> tuple[Step, ...]:
        """Zero-config Docker CI DAG.

        Pulls the base image (warmup), builds the image, checks the version.
        All steps share the warmup so the base image pull is not repeated.

        Examples:
            >>> import harmont as hm
            >>> proj = hm.docker.project(version="24.0", image="ubuntu:22.04")
            >>> hm.pipeline(list(proj.ci()))
        """
        command_with_args = ('echo', 'Hello World')
        return self.build(), self.version(), self.run(command_with_args=command_with_args)
  
def _make_docker(
    *,
    version: str,
    channel: str = "stable",
    path: str = ".",
    image: str | None = None,
    base: Step | None = None
) -> DockerToolchain:
    if not _VERSION_RE.match(version):
        msg = (
            f"hm.docker: invalid version is {version!r}\n" 
            "use a version like '20.10' or '24.0'"
        )
        raise ValueError(msg)

    if _parse_version(version) < _MIN_DOCKER_VERSION:
        msg = (
            f"hm.docker: version {version!r} is below the minimum "
            f"{'.'.join(str(p) for p in _MIN_DOCKER_VERSION)}\n"
            "  - install a newer docker version"
        )
        raise ValueError(msg)
    
    install_cmd = _docker_install_cmd_using_convenience_script(version=version, channel=channel)
    installed = make_install_chain(
        apt_packages=APT_PACKAGES,
        install_cmd=install_cmd,
        install_cache=CacheForever(env_keys=()),
        install_tag="docker",
        lang_tag="docker",
        image=image,
        base=base
    )
    return DockerToolchain(path=path, installed=installed)

def _make_docker_project(
    *,
    image: str = "hello-world",
    version: str = "24.0",
    channel: str = "stable",
    path: str = ".",
    cache: CachePolicy | None = None,
) -> DockerProject:
    """Build a DockerProject with a shared warmup step.

    Pulls ``image`` as the warmup, keyed on Dockerfile changes.
    """
    tc = _make_docker(
        path=path,
        channel=channel,
        version=version
    )

    if not path:
        path = "."

    dockerfile = f"{path}/Dockerfile" if path != "." else "Dockerfile"

    warmup_cache = (
        cache if cache is not None else CacheOnChange(paths=(dockerfile, f"{path}/**"))
    )

    warm = tc._emit(
        f"docker pull {image}",
        ":docker: warmup",
        cache=warmup_cache
    )

    return DockerProject(toolchain=tc, warmup=warm, path=path)

class DockerEntry:
    """Namespace for ``hm.docker.toolchain()`` and ``hm.docker.project()``."""

    @staticmethod
    def toolchain(
        *,
        version: str,
        channel: str = "stable",
        path: str  = ".",
    ) -> DockerToolchain:
        """Install the docker toolchain via a convenience script.

        Produces a ``DockerToolchain`` whose ``installed`` step is the 
        docker install step. Action methods on the toolchain attach leaves
        to ``installed``. Use ``project()`` instead when you want a 
        pre-built warmup step shared across build/version/run

        Args:
            path: Path to the crate or workspace root.
            version: the option to install a specific version
            channel: option to install from an alternative installation channel

        Returns:
            A ``DockerToolchain`` ready for action methods.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.docker.toolchain(version="")
            >>> hm.pipeline([tc.build(), tc.version(), tc.run()])
        
        """
        return _make_docker(
            version=version,
            channel=channel,
            path=path
        )

    @staticmethod
    def project(
        *,
        image: str,
        version: str,
        channel: str = "stable",
        path: str  = ".",
        cache: CachePolicy | None = None,
    ) -> DockerProject:
        """
        Installs the toolchain via the docker convenience script, warms a dependency cache
        keyed on ``Dockerfile`` and the build context ``{path}/**``, and returns a ``DockerProject`` whose ``.build()``, ``.run()``
        and ``.version()`` methods on that warmup step.

        Args:
            path: Path to the crate or workspace root.
            version: the option to install a specific version
            channel: option to install from an alternative installation channel
            cache: Override the warmup step's cache policy.defaults to 
                   ``CacheOnChange`` keyed on ``Dockerfile``

        Returns:
            A ``DockerProject`` exposing the common CI steps.

        Examples:
            >>> import harmont as hm
            >>> tc = hm.docker.project()
            >>> hm.pipeline([tc.build(), tc.version(), tc.run()])
        """
        return _make_docker_project(
            image=image,
            version=version,
            channel=channel,
            path=path,
            cache=cache
        )

docker: DockerEntry = DockerEntry()