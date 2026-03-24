FROM ubuntu:24.04

# System tools for code analysis
RUN apt-get update && apt-get install -y --no-install-recommends \
    git curl ca-certificates xz-utils \
    && rm -rf /var/lib/apt/lists/*

# ripgrep
RUN ARCH="$(uname -m)" && \
    curl -sL "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-${ARCH}-unknown-linux-musl.tar.gz" \
    | tar xz --strip-components=1 -C /usr/local/bin --wildcards '*/rg'

# tokei (code statistics)
RUN curl -sL https://github.com/XAMPPRocky/tokei/releases/download/v13.0.0-alpha.5/tokei-x86_64-unknown-linux-gnu.tar.gz \
    | tar xz -C /usr/local/bin

# uv (Python version + environment manager)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:$PATH"

# OpenCode — pinned version for stability
ARG OPENCODE_VERSION=0.1.0
RUN curl -fsSL https://opencode.ai/install | bash

# ps-agent-mcp binary (built externally, copied in)
COPY ps-agent-mcp /usr/local/bin/ps-agent-mcp
RUN chmod +x /usr/local/bin/ps-agent-mcp

# OpenCode configuration + agent prompt
COPY opencode.json /app/opencode.json
COPY .opencode/ /app/.opencode/

# Workspace for cloned repos and generated files
RUN mkdir /workspace
WORKDIR /workspace

# OpenCode server port
EXPOSE 4096

# Start OpenCode server
CMD ["opencode", "--server", "--port", "4096", "--config", "/app/opencode.json"]
