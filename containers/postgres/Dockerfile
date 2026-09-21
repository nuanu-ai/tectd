# syntax=docker/dockerfile:1.7
FROM --platform=linux/amd64 postgres:18.6-trixie@sha256:7341002d2b8c7c5bdd7542a671a95b36196c0b5b888daf454ae4fc33ba5346d7
ARG PGRDF_ARCHIVE=pgrdf-0.6.34-pg18-glibc-amd64.tar.gz
COPY ${PGRDF_ARCHIVE} /tmp/pgrdf.tar.gz
RUN test "$(wc -c </tmp/pgrdf.tar.gz)" = "10588401" \
    && echo "78b9efa983bc5b494f163a7896cd2260314830ac166e8ab2b04e549f91852b8f  /tmp/pgrdf.tar.gz" | sha256sum -c - \
    && mkdir /tmp/pgrdf \
    && tar -xzf /tmp/pgrdf.tar.gz -C /tmp/pgrdf --strip-components=1 \
    && cd /tmp/pgrdf \
    && sha256sum -c SHA256SUMS \
    && grep -F '"version": "0.6.34"' MANIFEST.json \
    && grep -F '"git_sha": "1b82842dcb60b74c060b0344310b47d1c84a8e8d"' MANIFEST.json \
    && install -m 0755 lib/pgrdf.so "$(pg_config --pkglibdir)/pgrdf.so" \
    && install -m 0644 share/extension/pgrdf* "$(pg_config --sharedir)/extension/" \
    && rm -rf /tmp/pgrdf /tmp/pgrdf.tar.gz
CMD ["postgres", "-c", "shared_preload_libraries=pgrdf"]
