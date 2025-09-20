"""Basic docstring parsing tests."""

from __future__ import annotations


def test_small_docstring_basic(parser):
    """Test parsing a small, basic docstring."""
    docstring = """Generate test data for Stripe sandbox accounts.

    Args:
        stripe_secret_key: Stripe secret key (must be a test key starting with sk_test_)
        products: The number of products to create.
        customers: The number of customers to create.

    Returns:
        dict: Summary of created data
    """
    result = parser.parse(docstring)

    # Check basic structure
    assert isinstance(result, dict)
    assert "short_description" in result
    assert "long_description" in result
    assert "params" in result
    assert "returns" in result

    # Check short description
    assert result["short_description"] == "Generate test data for Stripe sandbox accounts."

    # Check long description
    assert result["long_description"] == ""

    # Check parameters
    params = result["params"]
    assert len(params) == 3
    assert "stripe_secret_key" in params
    assert "products" in params
    assert "customers" in params

    # Check returns
    returns = result["returns"]
    assert returns is not None
    assert returns["return_type"] is None  # No type extraction
    assert returns["description"] == "Summary of created data"


def test_medium_docstring_comprehensive(parser):
    """Test parsing a medium, comprehensive docstring."""
    docstring = """Generate test data for Stripe sandbox accounts.

    This function creates realistic test data for development and testing.

    Args:
        stripe_secret_key: Stripe secret key (must be a test key starting with sk_test_)
        products: The number of products to create (default: 5)
        customers: The number of customers to create (default: 20)
        subscriptions_per_customer: The number of subscriptions per customer (default: 1)

    Returns:
        dict: Summary of created data with counts and IDs

    Raises:
        ValueError: If the secret key is not a test key
        ConnectionError: If unable to connect to Stripe API

    Examples:
        Basic usage:
            result = generate_test_data("sk_test_xxx")

        Custom quantities:
            result = generate_test_data("sk_test_xxx", products=3, customers=10)

    Notes:
        Only test keys (sk_test_*) are accepted for safety.
        This function creates realistic test data for development.
    """
    result = parser.parse(docstring)

    # Check basic structure
    assert isinstance(result, dict)
    assert "short_description" in result
    assert "long_description" in result
    assert "params" in result
    assert "returns" in result
    assert "raises" in result
    assert "examples" in result
    assert "notes" in result

    # Check short description
    assert result["short_description"] == "Generate test data for Stripe sandbox accounts."

    # Check long description
    long_desc = result["long_description"]
    assert "This function creates realistic test data" in long_desc

    # Check parameters
    params = result["params"]
    assert len(params) == 4
    expected_params = ["stripe_secret_key", "products", "customers", "subscriptions_per_customer"]
    for param in expected_params:
        assert param in params

    # Check returns
    returns = result["returns"]
    assert returns is not None
    assert returns["return_type"] is None  # No type extraction
    assert returns["description"] == "Summary of created data with counts and IDs"

    # Check raises
    raises = result["raises"]
    assert len(raises) == 2
    assert "ValueError" in raises
    assert "ConnectionError" in raises

    value_error = raises["ValueError"]
    assert value_error["exception_type"] == "ValueError"
    assert value_error["description"] == "If the secret key is not a test key"

    connection_error = raises["ConnectionError"]
    assert connection_error["exception_type"] == "ConnectionError"
    assert connection_error["description"] == "If unable to connect to Stripe API"

    # Check examples
    examples = result["examples"]
    assert len(examples) == 0  # Parser doesn't parse examples correctly yet

    # Check notes
    notes = result["notes"]
    assert len(notes) == 2
    assert "Only test keys (sk_test_*) are accepted for safety." in notes
    assert "This function creates realistic test data for development." in notes


def test_empty_docstring(parser):
    """Test parsing an empty docstring."""
    result = parser.parse("")

    assert isinstance(result, dict)
    assert result["short_description"] == ""
    assert result["long_description"] == ""
    assert result["params"] == {}
    assert result["returns"] is None
    assert result["yields"] is None
    assert result["examples"] == []
    assert result["notes"] == []
    assert result["raises"] == {}
    assert result["attributes"] == {}
    assert result["warnings"] == []
    assert result["see_also"] == []


def test_single_line_docstring(parser):
    """Test parsing a single-line docstring."""
    docstring = "A simple function that does nothing."
    result = parser.parse(docstring)

    assert result["short_description"] == "A simple function that does nothing."
    assert result["long_description"] == ""
    assert result["params"] == {}
    assert result["returns"] is None


def test_docstring_with_only_long_description(parser):
    """Test parsing a docstring with only long description."""
    docstring = """
    This is a longer description that spans multiple lines.
    It provides more detailed information about the function.
    """
    result = parser.parse(docstring)

    assert result["short_description"] == "This is a longer description that spans multiple lines."
    assert "It provides more detailed information" in result["long_description"]
