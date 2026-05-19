import pytest
import requests
from playwright.sync_api import expect


@pytest.fixture(scope="session")
def base_url() -> str:
    return "http://localhost:3000"


@pytest.fixture(scope="session")
def _check_server(base_url):
    try:
        r = requests.get(f"{base_url}/api/brooders", timeout=3)
    except requests.RequestException as e:
        pytest.skip(
            f"QuailSync server not reachable at {base_url} "
            f"({e!s}). Start it with `docker compose up` or `cargo run -p quailsync-server`."
        )
    if r.status_code != 200:
        pytest.skip(
            f"QuailSync server at {base_url}/api/brooders returned {r.status_code}; "
            f"expected 200."
        )


@pytest.fixture(scope="session")
def _check_dev_mode(_check_server, base_url):
    r = requests.get(f"{base_url}/api/dev/status", timeout=3)
    if r.status_code == 404:
        pytest.skip(
            "Dev mode is not enabled on the server. Set DEV_MODE=true in "
            "docker-compose.yml (or the server's environment) and restart."
        )
    if r.status_code != 200 or not r.json().get("dev_mode"):
        pytest.skip(f"/api/dev/status did not report dev_mode=true (got {r.status_code}: {r.text}).")


@pytest.fixture(autouse=True)
def seed_test_data(_check_dev_mode, base_url):
    r = requests.post(f"{base_url}/api/dev/seed", timeout=15)
    assert r.status_code == 200, f"/api/dev/seed failed: {r.status_code} {r.text}"
    yield
    requests.post(f"{base_url}/api/dev/restore", timeout=15)


@pytest.fixture
def page(browser, base_url, seed_test_data):
    # Depending on seed_test_data guarantees the seed POST completes before
    # we open the browser, even though seed_test_data is autouse.
    context = browser.new_context(base_url=base_url)
    p = context.new_page()
    p.goto("/")
    expect(p.locator(".sidebar")).to_be_visible()
    # The router fades in the first page over ~180ms after hashchange. Wait
    # for one piece of dashboard content (the brooder panel id) so subsequent
    # assertions don't race the initial render.
    expect(p.locator("#d-brooder")).to_be_visible()
    yield p
    context.close()
