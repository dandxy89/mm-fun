# Aeron Media Driver Dockerfile
# Based on Java implementation of Aeron

FROM eclipse-temurin:21-jdk-alpine

# Install required packages
RUN apk add --no-cache curl bash

# Download Aeron all-in-one JAR with media driver
ARG AERON_VERSION=1.44.1
RUN mkdir -p /opt/aeron && \
    curl -L "https://repo1.maven.org/maven2/io/aeron/aeron-all/${AERON_VERSION}/aeron-all-${AERON_VERSION}.jar" \
    -o /opt/aeron/aeron-all.jar

# Create directory for Aeron shared memory
RUN mkdir -p /dev/shm/aeron

# Set environment variables for media driver configuration
ENV AERON_DIR=/dev/shm/aeron
ENV AERON_THREADING_MODE=SHARED
ENV AERON_TERM_BUFFER_LENGTH=16777216
ENV AERON_IPC_TERM_BUFFER_LENGTH=16777216
ENV AERON_PUBLICATION_LINGER_TIMEOUT=5000000000
ENV AERON_CLIENT_LIVENESS_TIMEOUT=10000000000

# Expose ports for UDP communication (if needed)
EXPOSE 40123-40125/udp

# Set working directory
WORKDIR /opt/aeron

# Copy Java launcher
COPY docker/aeronmd/MediaDriverLauncher.java /opt/aeron/

# Compile Java launcher
RUN javac -cp /opt/aeron/aeron-all.jar /opt/aeron/MediaDriverLauncher.java

# Healthcheck - verify media driver is actually running by checking for cnc.dat file
# The cnc.dat (Command and Control) file confirms the media driver is operational
HEALTHCHECK --interval=5s --timeout=3s --start-period=10s --retries=3 \
    CMD test -f /dev/shm/aeron/cnc.dat || exit 1

# Run media driver with custom launcher and required module opens for Java 21
CMD ["java", "--add-opens", "java.base/sun.nio.ch=ALL-UNNAMED", "-cp", "/opt/aeron:/opt/aeron/aeron-all.jar", "MediaDriverLauncher"]
