"""Enable ``python -m thoxa`` (used by the Zed extension to launch the LSP)."""
from .cli import main

raise SystemExit(main())
