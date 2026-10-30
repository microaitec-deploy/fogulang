docker build -f docker/Dockerfile --output dist .

docker build -f docker/Dockerfile --build-arg "TARGETS=linux-x64 linux-arm64" --output dist .
docker build -f docker/Dockerfile --build-arg "TARGETS=linux-armv7" --output dist .