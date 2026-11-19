// www.franken-textiles.com answers with a permanent redirect to the same path on franken-textiles.com.
export default {
  fetch(request) {
    const url = new URL(request.url);
    url.protocol = "https:";
    url.hostname = "franken-textiles.com";
    url.port = "";
    return Response.redirect(url.href, 301);
  },
};
