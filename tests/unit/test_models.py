"""
Unit tests for Ryx model functionality (no database required).
"""

import pytest
import sys
from unittest.mock import patch

# Mock ryx_core for unit tests - will be provided by conftest.py
# The mock_core fixture in conftest.py handles this


from ryx.fields import (
    AutoField, BigIntField, BooleanField, CharField,
    DateField, DateTimeField, ForeignKey, IntField, TextField, UUIDField,
)
from ryx.models import Model, Options, _to_table_name
from ryx.queryset import QuerySet, _parse_lookup_key
from ryx.exceptions import DoesNotExist, MultipleObjectsReturned


class TestTableNameDerivation:
    """Test the CamelCase → snake_case plural conversion."""

    @pytest.mark.parametrize("input_name,expected", [
        ("Post", "posts"),
        ("PostComment", "post_comments"),
        ("User", "users"),
        ("Status", "statuses"),  # Words ending in 's' get 'es'
        ("UserProfileImage", "user_profile_images"),
        ("API", "apis"),
        ("HTTPResponse", "http_responses"),
    ])
    def test_table_name_conversion(self, input_name, expected):
        assert _to_table_name(input_name) == expected


class TestModelMetaclass:
    """Test model metaclass functionality."""

    def test_basic_model_creation(self):
        class TestModel(Model):
            name = CharField(max_length=100)
            age = IntField()

        assert hasattr(TestModel, '_meta')
        assert TestModel._meta.table_name == "test_models"
        assert 'name' in TestModel._meta.fields
        assert 'age' in TestModel._meta.fields
        assert TestModel._meta.pk_field is not None
        assert TestModel._meta.pk_field.attname == 'id'

    def test_custom_table_name(self):
        class CustomTableModel(Model):
            class Meta:
                table_name = "my_custom_table"
            name = CharField(max_length=100)

        assert CustomTableModel._meta.table_name == "my_custom_table"

    def test_abstract_model(self):
        class AbstractModel(Model):
            class Meta:
                abstract = True
            name = CharField(max_length=100)

        # Abstract models shouldn't have a table name or be processed fully
        assert AbstractModel._meta.abstract is True

    def test_unique_together(self):
        class UniqueModel(Model):
            class Meta:
                unique_together = [("field1", "field2")]
            field1 = CharField(max_length=50)
            field2 = IntField()

        assert UniqueModel._meta.unique_together == [("field1", "field2")]

    def test_indexes(self):
        from ryx.models import Index

        class IndexedModel(Model):
            class Meta:
                indexes = [
                    Index(fields=["name"], name="name_idx"),
                    Index(fields=["created_at"], name="date_idx", unique=True),
                ]
            name = CharField(max_length=100)
            created_at = DateTimeField()

        assert len(IndexedModel._meta.indexes) == 2
        assert IndexedModel._meta.indexes[0].name == "name_idx"
        assert IndexedModel._meta.indexes[1].unique is True

    def test_constraints(self):
        from ryx.models import Constraint

        class ConstrainedModel(Model):
            class Meta:
                constraints = [
                    Constraint(check="age >= 0", name="age_positive"),
                ]
            age = IntField()

        assert len(ConstrainedModel._meta.constraints) == 1
        assert ConstrainedModel._meta.constraints[0].check == "age >= 0"

    def test_per_model_exceptions(self):
        class TestModel(Model):
            name = CharField(max_length=100)

        assert hasattr(TestModel, 'DoesNotExist')
        assert hasattr(TestModel, 'MultipleObjectsReturned')
        assert issubclass(TestModel.DoesNotExist, DoesNotExist)
        assert issubclass(TestModel.MultipleObjectsReturned, MultipleObjectsReturned)

    def test_inheritance(self):
        class BaseModel(Model):
            class Meta:
                abstract = True
            created_at = DateTimeField(auto_now_add=True)

        class ChildModel(BaseModel):
            name = CharField(max_length=100)

        # Child should inherit fields from base
        assert 'created_at' in ChildModel._meta.fields
        assert 'name' in ChildModel._meta.fields
        assert ChildModel._meta.pk_field is not None


class TestModelInstance:
    """Test model instance creation and behavior."""

    def test_instance_creation(self):
        class TestModel(Model):
            name = CharField(max_length=100)
            age = IntField(default=25)

        instance = TestModel(name="John", age=30)
        assert instance.name == "John"
        assert instance.age == 30

    def test_default_values(self):
        class TestModel(Model):
            name = CharField(max_length=100, default="Unknown")
            age = IntField(default=25)

        instance = TestModel()
        assert instance.name == "Unknown"
        assert instance.age == 25

    def test_pk_property(self):
        class TestModel(Model):
            custom_id = IntField(primary_key=True)
            name = CharField(max_length=100)

        instance = TestModel(custom_id=42, name="Test")
        assert instance.pk == 42

    def test_from_row(self):
        class TestModel(Model):
            name = CharField(max_length=100)
            age = IntField()

        row = {"id": 1, "name": "John", "age": 30}
        instance = TestModel._from_row(row)
        assert instance.pk == 1
        assert instance.name == "John"
        assert instance.age == 30

    def test_invalid_field_assignment(self):
        class TestModel(Model):
            name = CharField(max_length=100)

        with pytest.raises(TypeError, match="unexpected keyword argument"):
            TestModel(name="John", invalid_field="value")


class TestManager:
    """Test the default model manager."""

    def test_manager_creation(self):
        class TestModel(Model):
            name = CharField(max_length=100)

        assert hasattr(TestModel, 'objects')
        assert hasattr(TestModel.objects, 'get_queryset')

    def test_queryset_methods(self):
        class TestModel(Model):
            name = CharField(max_length=100)

        qs = TestModel.objects.all()
        assert isinstance(qs, QuerySet)
        # QuerySet stores model internally as _model
        assert qs._model == TestModel

        # Test proxy methods exist
        assert hasattr(TestModel.objects, 'filter')
        assert hasattr(TestModel.objects, 'exclude')
        assert hasattr(TestModel.objects, 'order_by')


class TestOptions:
    """Test the Options class."""

    def test_options_creation(self):
        """Test Options with custom Meta attributes."""
        class Meta:
            table_name = "custom_table"
            ordering = ["-created_at"]
            unique_together = [("a", "b")]

        opts = Options(Meta, "TestModel")
        assert opts.table_name == "custom_table"
        assert opts.ordering == ["-created_at"]
        assert opts.unique_together == [("a", "b")]

    def test_options_default_table_name(self):
        """Test Options derives table name from model if not in Meta."""
        opts = Options(None, "TestModel")
        # Table name should be derived from model name
        assert opts.table_name is not None