(function () {
  var path = window.location.pathname.replace(/^\/ToolR\/?/i, "");
  window.location.replace(
    "https://toolr.readthedocs.io/stable/" + path + window.location.search + window.location.hash
  );
})();
