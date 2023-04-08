FROM alpine-versatiles

# fetch frontend
RUN curl -Ls "https://github.com/versatiles-org/versatiles-frontend/releases/latest/download/frontend.br.tar" > frontend.br.tar

# open port
EXPOSE 8088

# Run the web service on container startup.
CMD versatiles serve --auto-shutdown 1000 -i "0.0.0.0" -p 8088 -s ./frontend.br.tar \
    "[osm]https://storage.googleapis.com/versatiles/download/planet/planet-20230227.versatiles" \
    "[vg250_gem]https://storage.googleapis.com/versatiles/download/geometries_vg250_gem_20201231.versatiles"
