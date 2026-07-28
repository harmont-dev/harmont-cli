"""Scala toolchain abstraction .

Public surface lives on the module-level singleton ``scala``:

  hm.scala.toolchain(...)  -> ScalaToolchain  (install-only)
  hm.scala.project(...)    -> ScalaProject    (full CI DAG)
"""

from __future__ import annotations

import re, shlex
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Self, overload

from ._toolchain import advance_install, make_install_chain
from .cache import CacheForever, CacheOnChange

if TYPE_CHECKING:
    from ._step import Step
    from .cache import CachePolicy

APT_PACKAGES = ("curl", "ca-certificates", "openjdk-17-jdk-headless")
_VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$") # 3.3.8

def _parse_version(version: str) -> tuple[int, ...]:
  return tuple(int(p) for p in version.split("."))

def coursier_install_cmd(version: str = "3.3.8") -> str:
  return (
    f"curl -fL https://github.com/coursier/launchers/raw/master/cs-x86_64-pc-linux.gz | gzip -d > cs && "
    f"chmod +x cs && "
    f"./cs setup --yes && "
    f"./cs install scala:{version} && "
    f"./cs list && "
    f"ln -sf /root/.local/share/coursier/bin/* /usr/local/bin/"
  )

def _compile_cmd(*, query: str | None = None, test: bool = False) -> str:
  scope = ""
  if query:
    scope += f"{query} / "
  if test:
    scope += f"Test / " 
  return f"sbt {scope}compile"

#  sbt [query / ] test [testname1 testname2] [ -- options ]
def _test_cmd(
    *, 
    query: str | None = None, 
    testnames: tuple[str, ...] = (), 
    options: dict[str, Any] | None = None
) -> str:
  scope = ""
  test_names = ""
  opts = ""
  if query:
    scope += f"{query} /"
  if testnames:
    test_names += " ".join(testname for testname in testnames)
  if options:
    opts += "--"
    opts += " ".join(f'{key} {shlex.quote(value)}' for key, value in options.items())  
  return f"sbt {scope}test{test_names} {opts}"

def _fmt_cmd(check: bool = False) -> str:
  return "sbt scalafmtCheckAll" if check else "sbt scalafmtAll"

# TODO - chain the 3 compile, test, fmt  together, use clippy as reference for scala fix, use shlex.quote() for this 
# TODO - _scalafix_cmd(*, check, rules, diff)  # needs flags

@dataclass(frozen=True)
class ScalaToolchain:
  """Scala toolchain install chain — constructed via ``hm.scala()`` with no ``path``.

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
    env: dict[str, str] | None = None
  ) -> Self:
    """Append a post-install command and return an advanced toolchain; chainable.

    Use for prep steps the toolchain's actions must depend on but that the SDK
    does not model natively — code generation, fixtures, extra tooling. The
    returned object's action methods fork from this step.

    Examples:
        >>> import harmont as hm
        >>> proj = hm.scala(path=".").setup("cs install scalafix") #install linting tool
    """
    return advance_install(self, cmd, cwd=cwd, label=label, cache=cache, env=env)

  def _wrap(self, sbt_cmd: str) -> str:
    return f"export PATH=\"$PATH:$HOME/.local/share/coursier/bin\" && cd {self.path} && {sbt_cmd}"
  
  def _emit(self, sbt_cmd: str, default_label: str, **kw: Any) -> Step:
    if kw.get("label") is None:
      kw["label"] = default_label
    return self.installed.sh(self._wrap(sbt_cmd), **kw)

  def compile(
    self,
    query: str | None = None,
    test: bool = False,
    **kw: Any
  ) -> Step:
    """Compile the project ( sbt compile ) or compile tests ( sbt 'Test/ compile')"""
    cmd = _compile_cmd(query=query,test=test)
    return self._emit(
      cmd,
      f":scala: {self.path} compile",
      **kw
    )
  
  def test(
    self,
    query: str | None = None,
    testnames: tuple[str, ...] = (),
    options: dict[str, Any] | None = None,
    **kw: Any
  ) -> Step:
    """Run tests the project ( sbt test ) or compile tests ( sbt 'Test/ compile')"""
    cmd = _test_cmd(
      query=query,
      testnames=testnames,
      options=options
    )
    return self._emit(
      cmd,
      f":scala: {self.path} test",
      **kw
    )
  
  def fmt(
      self,
    check: bool = False,
    **kw
  ) -> Step:
    """Run fmt across the project ( sbt scalafmtAll ) or compile tests ( sbt 'Test/ compile')"""
    cmd = _fmt_cmd(check=check)
    return self._emit(
      cmd,
      f":scala: {self.path} format",
      **kw
    )
  
  def warmup(self, **kw: Any) -> Step:
    """Pre-download dependencies and warm sbt so later steps reuse the cache.
       Used internally by ``hm.scala.project()``.
    """
    return self._emit("sbt update", ":scala: warmup",**kw)


@dataclass(frozen=True)
class ScalaProject:
  """High-level Scala CI DAG --- constructed via ``hm.scala.project()``.

  Action methods (``build``, ``test``, ``fmt``) attach leaves to the shared warmup step so 
  dependency compilation is reused. ``ci()`` returns the standard DAG in one call.
  """
  path: str
  toolchain: ScalaToolchain
  warmup: Step

  def setup(
      self,
      cmd: str,
      *,
      cwd: str | None = None,
      label: str | None = None,
      cache: CachePolicy | None = None,
      env: dict[str, str] | None = None,
  ) -> Self:
    """Append a post-install command and return an advanced project; chainable.

    Use for prep steps the toolchain's actions must depend on but that the SDK
    does not model natively — code generation, fixtures, extra tooling. The 
    returned object's action methods fork from this step.

    Examples:
        >>> import harmont as hm
        >>> tc = hm.scala(path=".").setup("cs install scalafix ") 
    """
    return advance_install(self, cmd, cwd=cwd, label=label, cache=cache, env=env)
  
  def _wrap(self, cmd: str) -> str:
    return f"cd {self.path} && {cmd}"
  
  def _emit(self, cmd: str, default_label: str, **kw: Any) -> Step:
    if kw.get("label") is None:
      kw["label"] = default_label
    return self.warmup.sh(self._wrap(cmd), **kw)
  
  def compile(
    self,
    query: str | None = None,
    test: bool = False,
    **kw: Any
  ) -> Step:
    """Compile the project ( sbt compile ) or compile tests ( sbt 'Test/ compile')"""
    cmd = _compile_cmd(query=query,test=test)
    return self._emit(
      cmd,
      f":scala: {self.path} sbt compile",
      **kw
    )
  
  def test(
    self,
    query: str | None = None,
    testnames: tuple[str, ...] = (),
    options: dict[str, Any] | None = None,
    **kw: Any
  ) -> Step:
    """Run tests the project ( sbt test ) or compile tests ( sbt 'Test/ compile')"""
    cmd = _test_cmd(
      query=query,
      testnames=testnames,
      options=options
    )
    return self._emit(
      cmd,
      f":scala: {self.path} sbt test",
      **kw
    )
  
  def fmt(
      self,
    check: bool = False,
    **kw
  ) -> Step:
    """Check formatting (``sbt scalafmtAll``). Chains off the toolchain
        install so it runs in parallel with the warmup."""
    cmd = _fmt_cmd(check=check)
    return self._emit(
      cmd,
      f":scala: {self.path} sbt format",
      **kw
    )
    # return self.toolchain.fmt(check=check, **kw)

  def ci(self) -> tuple[Step, ...]:
    """The zero-config Scala CI DAG. compile and test share the warmup;
    fmt runs off the toolchain install step, in parallel.

    Examples:
        >>> import harmont as hm
        >>> proj = hm.scala.project()
        >>> hm.pipeline(list(proj.ci()))
    """
    return self.compile(), self.test(), self.fmt()

def _make_scala(
  *,
  path: str = ".",
  version: str = "3.3.8",
  image: str | None = None,
  components: tuple[str, ...] = (), # add components - cs install deps/libs
  base: Step | None = None,
) -> ScalaToolchain:
  if not _VERSION_RE.match(version):
    msg = f'hm.scala: invalid version {version!r}\n -> use a valid scala version'
    raise ValueError(msg)

  if _parse_version(version) < (2, 13, 0):
    msg = f'hm.scala: version lower {version!r}\n -> use a scala version 2.13 and above'
    raise ValueError(msg)
  
  installed = make_install_chain(
    apt_packages=APT_PACKAGES,
    install_cmd=coursier_install_cmd(version), #  components to be passed for extra install
    install_cache=CacheForever(env_keys=()),
    lang_tag="scala",
    install_tag="install",
    image=image,
    base=base,
  )
  return ScalaToolchain(path=path, installed=installed)

def _make_scala_project(
    *,
    path: str = ".",
    version: str = "3.3.8",
    image: str | None = None,
    components: tuple[str, ...] = (), # scalafix and scalafmtAll
    base: Step | None = None,
    cache: CachePolicy | None = None
) -> ScalaProject:
  tc = _make_scala(
    path=path,
    version=version,
    image=image,
    components=components,
    base=base
  )

  if not path:
    path = "."

  build_file_path = f"{path}/build.sbt" if path != "." else "build.sbt"
  properties_file = f"{path}/project/build.properties" if path != "." else "project/build.properties"
  scala_main_glob = f"{path}/**/src/main/*.scala" if path != "." else "**/src/main/*.scala"
  scala_test_glob = f"{path}/**/src/test/*.scala" if path != "." else "**/src/test/*.scala"

  warmup_cache = (
    cache if cache is not None else CacheOnChange(paths=(build_file_path, scala_main_glob, scala_test_glob, properties_file))
  )

  warm = tc._emit(
    "sbt update",
    f":scala: sbt warmup",
    cache=warmup_cache
  )

  return ScalaProject(path=path, toolchain=tc, warmup=warm)


class ScalaEntry:
    """Namespace for ``hm.scala.toolchain()`` and ``hm.scala.project()``."""
    
    @staticmethod
    def toolchain(
      *,
      path: str = ".",
      scala_version: str = "3.3.8",
      image: str | None = None,
      components: tuple[str, ...] = (),
      base: Step | None = None,

    ) -> ScalaToolchain: 
      """Install the Scala toolchain via coursier.

      Produces a ``ScalaToolchain`` whose ``installed`` step is the
      coursier step. Action methods on the toolchain attach leaves
      to ``installed``. Use ``project()`` instead when you want a pre-built
      warmup step shared across test/clippy/fmt

      Args:
        path: Path to the workspace root.
        version: a pinned version ``"3.3.8"``
        image: Local-mode Docker base image override.
        components: scala dependencies/libraries to install alongside the toolchain.
        base: Existing ``Step`` to attach the toolcain install to instead of emitting 
            a fresh apt-base step. Use to share one apt-base across multiple toolchains. 

      Returns:
        A ``ScalaToolchain`` ready for action methods.

      Examples:
          >>> import harmont as hm
          >>> tc = hm.rust.toolchain(version="3.3.8")
          >>> hm.pipeline([tc.test(), tc.clippy()])
      """

      return _make_scala(
        path=path,
        version=scala_version,
        image=image,
        base=base
      )

    @staticmethod
    def project(
      *,
      path: str = ".",
      scala_version: str = "3.3.8",
      image: str | None = None,
      components: tuple[str, ...] = (),
      base: Step | None = None,
      cache: CachePolicy | None = None,
    ) -> ScalaProject:
      """Build a high-level Scala CI DAG.

      Installs the toolchain via coursier, warms a dependency cache keyed on
      ``build.sbt``, and returns a ``ScalaProject`` whose ``.test()``, ``fmt()``
      methods build on that warmup step so dependency compilation is shared.

      Args:
        path: Path to workspace root.
        version: a pinned version ``"3.3.8"``
        image: Local-mode Docker base image override.
        components: scala dependencies/libraries to install alongside the toolchain.
        base: Existing ``Step`` to attach the toolcain install to instead of emitting
            a fresh coursier step
        cache: Override the warmup step's cache policy. Defaults to ``CacheOnChange``
              keyed on ``build.sbt``, ``project/build.properties``,``**/*.scala``

      Returns:
         A ``ScalaProject`` exposing the common CI steps.

      Examples:
        >>> import harmont as hm
        >>> proj = hm.scala.project()
        >>> hm.group([proj.test(), proj.compile(), proj.fmt()])
          
      """

      return _make_scala_project(
        path=path,
        version=scala_version,
        image=image,
        components=components,
        base=base,
        cache=cache
      )
     
scala: ScalaEntry = ScalaEntry()