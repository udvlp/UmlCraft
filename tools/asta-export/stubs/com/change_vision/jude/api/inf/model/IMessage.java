// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface IMessage extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract boolean isReturnMessage();
    public abstract void setAsynchronous(boolean p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setOperation(com.change_vision.jude.api.inf.model.IOperation p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setArgument(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setReturnValue(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setGuard(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
