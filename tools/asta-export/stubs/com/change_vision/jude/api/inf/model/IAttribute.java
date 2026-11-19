// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IAttribute extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract com.change_vision.jude.api.inf.model.IClass getType();
    public abstract void setTypeExpression(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setInitialValue(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setChangeable(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setAggregation() throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setComposite() throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setStatic(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setMultiplicityString(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setDerived(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setNavigability(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setName(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setVisibility(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
