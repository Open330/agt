#!/usr/bin/env python3
"""
Agent Skills 설치 스크립트

이 레포지토리의 스킬들을 Claude Code에 설치합니다.

사용법:
    # 모든 스킬 설치
    python install.py

    # 특정 스킬만 설치
    python install.py context-manager multi-llm-agent

    # prefix/postfix 추가
    python install.py --prefix "my-" --postfix "-dev"

    # 설치 제거
    python install.py --uninstall
"""

import argparse
import os
import shutil
import sys
from pathlib import Path
from typing import Optional


# 기본 설정
DEFAULT_TARGET_DIR = Path.home() / ".claude" / "skills"
SKILL_MARKER_FILE = "SKILL.md"

# 제외할 디렉토리
EXCLUDE_DIRS = {".git", ".agents", "__pycache__", "node_modules", ".venv", "venv"}


class SkillInstaller:
    """스킬 설치 관리자"""

    def __init__(
        self,
        source_dir: Path,
        target_dir: Path,
        prefix: str = "",
        postfix: str = "",
        use_symlink: bool = True,
        verbose: bool = True
    ):
        self.source_dir = source_dir.resolve()
        self.target_dir = target_dir.resolve()
        self.prefix = prefix
        self.postfix = postfix
        self.use_symlink = use_symlink
        self.verbose = verbose

    def log(self, message: str, level: str = "info"):
        """로그 출력"""
        if not self.verbose and level == "debug":
            return

        icons = {
            "info": "ℹ️ ",
            "success": "✅",
            "warning": "⚠️ ",
            "error": "❌",
            "debug": "🔍",
            "skip": "⏭️ "
        }
        icon = icons.get(level, "")
        print(f"{icon} {message}")

    def discover_skills(self) -> list[Path]:
        """레포지토리에서 스킬 디렉토리 탐색"""
        skills = []
        for item in self.source_dir.iterdir():
            if item.is_dir() and item.name not in EXCLUDE_DIRS:
                skill_file = item / SKILL_MARKER_FILE
                if skill_file.exists():
                    skills.append(item)
        return sorted(skills, key=lambda x: x.name)

    def get_installed_skills(self) -> dict[str, Path]:
        """현재 설치된 스킬 조회 (이 레포지토리에서 설치된 것만)"""
        installed = {}
        if not self.target_dir.exists():
            return installed

        for item in self.target_dir.iterdir():
            if item.is_symlink():
                target = item.resolve()
                # 이 레포지토리에서 설치된 스킬인지 확인
                try:
                    target.relative_to(self.source_dir)
                    installed[item.name] = target
                except ValueError:
                    pass  # 다른 곳에서 설치된 스킬
            elif item.is_dir():
                # 복사로 설치된 경우 마커 파일로 확인
                marker = item / ".installed_from"
                if marker.exists():
                    source_path = Path(marker.read_text().strip())
                    try:
                        source_path.relative_to(self.source_dir)
                        installed[item.name] = source_path
                    except ValueError:
                        pass

        return installed

    def get_skill_name(self, skill_path: Path) -> str:
        """스킬 이름 생성 (prefix/postfix 적용)"""
        base_name = skill_path.name
        return f"{self.prefix}{base_name}{self.postfix}"

    def install_skill(self, skill_path: Path, dry_run: bool = False) -> bool:
        """단일 스킬 설치"""
        skill_name = self.get_skill_name(skill_path)
        target_path = self.target_dir / skill_name

        # 이미 존재하는 경우
        if target_path.exists() or target_path.is_symlink():
            if target_path.is_symlink() and target_path.resolve() == skill_path.resolve():
                self.log(f"{skill_name}: 이미 설치됨 (동일한 경로)", "skip")
                return True

            self.log(f"{skill_name}: 이미 존재함, 덮어쓰기...", "warning")
            if not dry_run:
                if target_path.is_symlink() or target_path.is_file():
                    target_path.unlink()
                else:
                    shutil.rmtree(target_path)

        if dry_run:
            method = "symlink" if self.use_symlink else "copy"
            self.log(f"{skill_name}: {skill_path} -> {target_path} ({method})", "debug")
            return True

        # 대상 디렉토리 생성
        self.target_dir.mkdir(parents=True, exist_ok=True)

        try:
            if self.use_symlink:
                target_path.symlink_to(skill_path)
                self.log(f"{skill_name}: 심볼릭 링크 생성됨", "success")
            else:
                shutil.copytree(skill_path, target_path)
                # 설치 출처 마커 생성
                marker = target_path / ".installed_from"
                marker.write_text(str(skill_path))
                self.log(f"{skill_name}: 복사 완료", "success")
            return True
        except Exception as e:
            self.log(f"{skill_name}: 설치 실패 - {e}", "error")
            return False

    def uninstall_skill(self, skill_name: str, dry_run: bool = False) -> bool:
        """단일 스킬 제거"""
        target_path = self.target_dir / skill_name

        if not target_path.exists() and not target_path.is_symlink():
            self.log(f"{skill_name}: 설치되어 있지 않음", "skip")
            return True

        if dry_run:
            self.log(f"{skill_name}: 제거 예정", "debug")
            return True

        try:
            if target_path.is_symlink() or target_path.is_file():
                target_path.unlink()
            else:
                shutil.rmtree(target_path)
            self.log(f"{skill_name}: 제거됨", "success")
            return True
        except Exception as e:
            self.log(f"{skill_name}: 제거 실패 - {e}", "error")
            return False

    def install_all(
        self,
        skill_names: Optional[list[str]] = None,
        dry_run: bool = False
    ) -> tuple[int, int]:
        """스킬 설치"""
        available_skills = self.discover_skills()

        if not available_skills:
            self.log("설치할 스킬이 없습니다.", "warning")
            return 0, 0

        # 특정 스킬만 선택
        if skill_names:
            skill_map = {s.name: s for s in available_skills}
            selected_skills = []
            for name in skill_names:
                if name in skill_map:
                    selected_skills.append(skill_map[name])
                else:
                    self.log(f"'{name}' 스킬을 찾을 수 없습니다.", "warning")
            available_skills = selected_skills

        if not available_skills:
            return 0, 0

        success_count = 0
        fail_count = 0

        self.log(f"\n{'=' * 50}")
        self.log(f"설치 대상: {len(available_skills)}개 스킬")
        self.log(f"설치 경로: {self.target_dir}")
        if self.prefix or self.postfix:
            self.log(f"이름 형식: {self.prefix}<skill-name>{self.postfix}")
        self.log(f"설치 방식: {'심볼릭 링크' if self.use_symlink else '복사'}")
        if dry_run:
            self.log("모드: DRY RUN (실제 설치 없음)", "warning")
        self.log(f"{'=' * 50}\n")

        for skill_path in available_skills:
            if self.install_skill(skill_path, dry_run):
                success_count += 1
            else:
                fail_count += 1

        self.log(f"\n{'=' * 50}")
        self.log(f"설치 완료: {success_count}개 성공, {fail_count}개 실패")
        self.log(f"{'=' * 50}")

        return success_count, fail_count

    def uninstall_all(
        self,
        skill_names: Optional[list[str]] = None,
        dry_run: bool = False
    ) -> tuple[int, int]:
        """스킬 제거"""
        installed = self.get_installed_skills()

        if not installed:
            self.log("이 레포지토리에서 설치된 스킬이 없습니다.", "info")
            return 0, 0

        # 특정 스킬만 선택
        if skill_names:
            # prefix/postfix 적용된 이름으로 변환
            target_names = set()
            for name in skill_names:
                full_name = f"{self.prefix}{name}{self.postfix}"
                target_names.add(full_name)
                target_names.add(name)  # 원본 이름도 시도

            installed = {k: v for k, v in installed.items() if k in target_names}

        if not installed:
            self.log("제거할 스킬이 없습니다.", "info")
            return 0, 0

        success_count = 0
        fail_count = 0

        self.log(f"\n{'=' * 50}")
        self.log(f"제거 대상: {len(installed)}개 스킬")
        if dry_run:
            self.log("모드: DRY RUN (실제 제거 없음)", "warning")
        self.log(f"{'=' * 50}\n")

        for skill_name in installed:
            if self.uninstall_skill(skill_name, dry_run):
                success_count += 1
            else:
                fail_count += 1

        self.log(f"\n{'=' * 50}")
        self.log(f"제거 완료: {success_count}개 성공, {fail_count}개 실패")
        self.log(f"{'=' * 50}")

        return success_count, fail_count

    def list_skills(self):
        """스킬 목록 출력"""
        available = self.discover_skills()
        installed = self.get_installed_skills()

        print(f"\n{'=' * 60}")
        print("📦 사용 가능한 스킬")
        print(f"{'=' * 60}")

        if not available:
            print("  (없음)")
        else:
            for skill_path in available:
                name = skill_path.name
                target_name = self.get_skill_name(skill_path)
                status = ""

                # 설치 상태 확인
                if target_name in installed:
                    status = " ✅ 설치됨"
                elif name in installed:
                    status = f" ✅ 설치됨 (as '{name}')"

                # SKILL.md에서 설명 추출
                desc = self._get_skill_description(skill_path)
                print(f"\n  📁 {name}{status}")
                if desc:
                    print(f"     {desc}")

        print(f"\n{'=' * 60}")
        print(f"설치 경로: {self.target_dir}")
        if self.prefix or self.postfix:
            print(f"이름 형식: {self.prefix}<skill-name>{self.postfix}")
        print(f"{'=' * 60}\n")

    def _get_skill_description(self, skill_path: Path) -> str:
        """SKILL.md에서 description 추출"""
        skill_file = skill_path / SKILL_MARKER_FILE
        try:
            content = skill_file.read_text()
            # YAML frontmatter에서 description 추출
            if content.startswith("---"):
                end = content.find("---", 3)
                if end > 0:
                    frontmatter = content[3:end]
                    for line in frontmatter.split("\n"):
                        if line.startswith("description:"):
                            desc = line[12:].strip()
                            if len(desc) > 80:
                                desc = desc[:77] + "..."
                            return desc
        except Exception:
            pass
        return ""


def main():
    parser = argparse.ArgumentParser(
        description="Agent Skills 설치 스크립트",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
예시:
  # 모든 스킬 설치
  python install.py

  # 특정 스킬만 설치
  python install.py context-manager multi-llm-agent

  # prefix 추가하여 설치 (예: my-context-manager)
  python install.py --prefix "my-"

  # postfix 추가하여 설치 (예: context-manager-dev)
  python install.py --postfix "-dev"

  # 다른 경로에 설치
  python install.py --target-dir ~/.claude/skills-dev

  # 심볼릭 링크 대신 복사
  python install.py --copy

  # 설치 미리보기 (dry-run)
  python install.py --dry-run

  # 설치된 스킬 제거
  python install.py --uninstall

  # 특정 스킬만 제거
  python install.py --uninstall context-manager

  # 스킬 목록 확인
  python install.py --list
        """
    )

    parser.add_argument(
        "skills",
        nargs="*",
        help="설치/제거할 스킬 이름 (지정하지 않으면 모든 스킬)"
    )

    parser.add_argument(
        "--prefix",
        default="",
        help="스킬 이름 앞에 추가할 접두사 (예: 'my-' -> my-context-manager)"
    )

    parser.add_argument(
        "--postfix", "--suffix",
        default="",
        dest="postfix",
        help="스킬 이름 뒤에 추가할 접미사 (예: '-dev' -> context-manager-dev)"
    )

    parser.add_argument(
        "--target-dir", "-t",
        type=Path,
        default=DEFAULT_TARGET_DIR,
        help=f"설치 대상 디렉토리 (기본값: {DEFAULT_TARGET_DIR})"
    )

    parser.add_argument(
        "--source-dir", "-s",
        type=Path,
        default=Path(__file__).parent,
        help="스킬 소스 디렉토리 (기본값: 이 스크립트 위치)"
    )

    parser.add_argument(
        "--copy", "-c",
        action="store_true",
        help="심볼릭 링크 대신 파일 복사 사용"
    )

    parser.add_argument(
        "--dry-run", "-n",
        action="store_true",
        help="실제 설치/제거 없이 미리보기만"
    )

    parser.add_argument(
        "--uninstall", "-u",
        action="store_true",
        help="스킬 제거 모드"
    )

    parser.add_argument(
        "--list", "-l",
        action="store_true",
        help="사용 가능한 스킬 목록 출력"
    )

    parser.add_argument(
        "--quiet", "-q",
        action="store_true",
        help="최소한의 출력만"
    )

    args = parser.parse_args()

    installer = SkillInstaller(
        source_dir=args.source_dir,
        target_dir=args.target_dir,
        prefix=args.prefix,
        postfix=args.postfix,
        use_symlink=not args.copy,
        verbose=not args.quiet
    )

    # 목록 출력
    if args.list:
        installer.list_skills()
        return 0

    # 제거 모드
    if args.uninstall:
        success, fail = installer.uninstall_all(
            skill_names=args.skills if args.skills else None,
            dry_run=args.dry_run
        )
        return 0 if fail == 0 else 1

    # 설치 모드
    success, fail = installer.install_all(
        skill_names=args.skills if args.skills else None,
        dry_run=args.dry_run
    )
    return 0 if fail == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
